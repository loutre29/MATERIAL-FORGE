//! Material Forge as an MCP server: Claude (the Claude desktop app or
//! Claude Code) textures the open model through tools, on the user's own
//! Claude plan, with no API key.
//!
//! `material_forge --mcp` is the server Claude starts (JSON-RPC over
//! stdio). It forwards each tool call to the Material Forge window the
//! user has open, over a local socket (`<data dir>/mcp.json` holds its
//! port and a token). It never starts the app itself: a copy started by
//! the Store build of the Claude app would run inside its package and
//! write its settings there.
//!
//! In the app a request runs as one typed in the Claude panel. Where the
//! panel would ask the API (`muse::run`), the question (instructions on
//! the first message, facts, images) goes back to Claude as the tool's
//! result, and Claude's `answer` call carries its reply. The self check
//! and the lighting check are asked the same way; the turn ends with the
//! panel's notes and a render of the result.

use eframe::egui;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// How long a tool call waits for the app before it answers that the work
/// goes on (MCP clients give up on calls that take minutes).
const WAIT: Duration = Duration::from_secs(45);
/// Text per part of a question (Claude Code takes about 25k tokens per
/// tool result).
const PAGE_TEXT: usize = 60_000;
/// Base64 image bytes per part (the Claude app takes 1 MB per result).
const PAGE_IMAGES: usize = 700_000;
/// Images per part.
const PAGE_COUNT: usize = 4;
/// The bridge says hello this often (the panel shows the link while it
/// does).
const HELLO: Duration = Duration::from_secs(15);

/// A tool call the window must handle (it owns the project).
pub(super) struct Cmd {
    pub tool: String,
    pub args: Value,
    pub reply: mpsc::Sender<Result<Vec<Value>, String>>,
}

#[derive(Default)]
struct State {
    inbox: Vec<Cmd>,
    /// A request from Claude is running (from `texture` to its end).
    turn: bool,
    /// The next question carries the instructions again (a new chat).
    resend: bool,
    /// The question not yet handed over, in parts.
    pages: VecDeque<Vec<Value>>,
    /// A question is out: its answer is awaited.
    question: bool,
    answer: Option<String>,
    /// The turn's end, not yet handed over.
    done: Option<Vec<Value>>,
    status: String,
    /// The last call or hello from a bridge.
    seen: Option<Instant>,
    ctx: Option<egui::Context>,
}

struct Link {
    st: Mutex<State>,
    cv: Condvar,
}

fn link() -> &'static Link {
    static L: OnceLock<Link> = OnceLock::new();
    L.get_or_init(|| Link { st: Mutex::new(State::default()), cv: Condvar::new() })
}

fn lock() -> MutexGuard<'static, State> {
    link().st.lock().unwrap_or_else(|e| e.into_inner())
}

pub(super) fn text(t: impl Into<String>) -> Value {
    json!({"type": "text", "text": t.into()})
}

/// An image for a tool result: JPEG, at most 1600 px long.
pub(super) fn image(img: &image::DynamicImage) -> Option<Value> {
    let img = if img.width().max(img.height()) > 1600 { img.resize(1600, 1600, image::imageops::FilterType::Triangle) } else { img.clone() };
    let mut jpg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 85).encode_image(&image::DynamicImage::ImageRgb8(img.to_rgb8())).ok()?;
    Some(json!({"type": "image", "data": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &jpg), "mimeType": "image/jpeg"}))
}

/// A Messages API image block as an MCP image.
fn from_block(b: &Value) -> Option<Value> {
    let data = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b["source"]["data"].as_str()?).ok()?;
    image(&image::load_from_memory(&data).ok()?)
}

// ------------------------------------------------------------ in the app

/// A request from Claude is running.
pub(super) fn turn_active() -> bool {
    lock().turn
}

/// A question waits for Claude's answer.
pub(super) fn asking() -> bool {
    let st = lock();
    st.question || !st.pages.is_empty()
}

/// A Claude app or Claude Code is linked (it said hello lately).
pub(super) fn linked() -> bool {
    let st = lock();
    st.turn || st.seen.is_some_and(|t| t.elapsed() < HELLO * 4)
}

/// UI checks: as if a Claude app had just said hello (or never had).
pub(super) fn set_linked(on: bool) {
    lock().seen = on.then(Instant::now);
}

pub(super) fn begin_turn() {
    let mut st = lock();
    st.turn = true;
    st.done = None;
    st.pages.clear();
    st.question = false;
    st.answer = None;
}

/// The request did not start.
pub(super) fn cancel_turn() {
    let mut st = lock();
    st.turn = false;
    st.pages.clear();
    st.question = false;
    link().cv.notify_all();
}

/// The request ended: `content` goes back to Claude.
pub(super) fn end_turn(content: Vec<Value>) {
    let mut st = lock();
    st.turn = false;
    st.pages.clear();
    st.question = false;
    st.done = Some(content);
    link().cv.notify_all();
}

/// The next question carries the instructions again.
pub(super) fn resend() {
    lock().resend = true;
}

/// The window's share of a frame: the calls waiting for it, and the
/// status line the tools report.
pub(super) fn take_commands(ctx: &egui::Context, status: &str) -> Vec<Cmd> {
    let mut st = lock();
    if st.ctx.is_none() {
        st.ctx = Some(ctx.clone());
    }
    st.status = status.to_string();
    std::mem::take(&mut st.inbox)
}

/// Asks Claude (on a worker thread, in place of the API): the last
/// message of `msgs` (its text and images), with the instructions on the
/// conversation's first message.
pub(super) fn ask_app(system: &str, msgs: &[Value], first: bool, stop: &AtomicBool) -> anyhow::Result<String> {
    let resend = std::mem::take(&mut lock().resend);
    let mut content = Vec::new();
    if first || resend {
        content.push(text(format!("MATERIAL FORGE'S INSTRUCTIONS for this texturing conversation. Follow them in every answer you give with the answer tool.\n\n{system}")));
    }
    // A new chat on a conversation under way: the first message's images
    // again (the instructions name them by their order there).
    if resend && !first {
        if let Some(blocks) = msgs.first().and_then(|m| m["content"].as_array()) {
            content.push(text("The images of the conversation's first message (the numbered views, the material sheet, the SURFACES image), in their order:"));
            content.extend(blocks.iter().filter(|b| b["type"] == "image").filter_map(from_block));
            content.push(text("THE NEW MESSAGE:"));
        }
    }
    for b in msgs.last().and_then(|m| m["content"].as_array()).into_iter().flatten() {
        match b["type"].as_str() {
            Some("text") => content.push(text(b["text"].as_str().unwrap_or_default())),
            Some("image") => content.extend(from_block(b)),
            _ => {}
        }
    }
    let pages = paginate(content);
    {
        let mut st = lock();
        st.pages = pages.into();
        st.question = true;
        st.answer = None;
        link().cv.notify_all();
    }
    let t0 = Instant::now();
    let mut st = lock();
    loop {
        if let Some(a) = st.answer.take() {
            return Ok(a);
        }
        if stop.load(Ordering::Relaxed) || !st.turn || t0.elapsed() > Duration::from_secs(3600) {
            st.pages.clear();
            st.question = false;
            anyhow::bail!("Cancelled.");
        }
        st = link().cv.wait_timeout(st, Duration::from_millis(200)).map(|r| r.0).unwrap_or_else(|e| e.into_inner().0);
    }
}

/// Splits a question in parts each tool result can carry (long text cut
/// at line ends; images in their order).
fn paginate(content: Vec<Value>) -> Vec<Vec<Value>> {
    struct Pages {
        pages: Vec<Vec<Value>>,
        chars: usize,
        bytes: usize,
        count: usize,
    }
    impl Pages {
        fn push(&mut self, b: Value) {
            let (c, y, n) = match b["type"].as_str() {
                Some("image") => (0, b["data"].as_str().map_or(0, str::len), 1),
                _ => (b["text"].as_str().map_or(0, str::len), 0, 0),
            };
            let full = self.chars + c > PAGE_TEXT || self.bytes + y > PAGE_IMAGES || self.count + n > PAGE_COUNT;
            if full && !self.pages.last().is_some_and(Vec::is_empty) {
                self.pages.push(Vec::new());
                (self.chars, self.bytes, self.count) = (0, 0, 0);
            }
            self.chars += c;
            self.bytes += y;
            self.count += n;
            self.pages.last_mut().unwrap().push(b);
        }
    }
    let mut p = Pages { pages: vec![Vec::new()], chars: 0, bytes: 0, count: 0 };
    for b in content {
        let Some(t) = b["text"].as_str().filter(|t| b["type"] == "text" && t.len() > PAGE_TEXT) else {
            p.push(b);
            continue;
        };
        // At line ends, as close to the part size as they come.
        let mut piece = String::new();
        for line in t.split_inclusive('\n') {
            if piece.len() + line.len() > PAGE_TEXT && !piece.is_empty() {
                p.push(text(std::mem::take(&mut piece)));
            }
            piece += line;
        }
        if !piece.is_empty() {
            p.push(text(piece));
        }
    }
    p.pages.retain(|p| !p.is_empty());
    p.pages
}

/// The local socket the bridges call (once; not in tests unless
/// MF_MCP_SERVE is set).
pub(super) fn serve() {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    if super::HEADLESS.load(Ordering::Relaxed) && std::env::var_os("MF_MCP_SERVE").is_none() {
        return;
    }
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => {
            eprintln!("MCP: no local socket ({e})");
            return;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let token = new_token();
    if let Some(d) = info_path().parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Err(e) = std::fs::write(info_path(), json!({"port": port, "token": token, "pid": std::process::id()}).to_string()) {
        eprintln!("MCP: mcp.json not written ({e})");
        return;
    }
    std::thread::spawn(move || {
        for s in listener.incoming().flatten() {
            let token = token.clone();
            std::thread::spawn(move || {
                let _ = connection(s, &token);
            });
        }
    });
}

fn info_path() -> PathBuf {
    let lib = crate::library::library_dir();
    lib.parent().map(Path::to_path_buf).unwrap_or(lib).join("mcp.json")
}

fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    (0..4)
        .map(|k| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(nanos ^ (k as u128) << 64);
            h.write_u32(std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}

/// One call from a bridge: a line in, a line out.
fn connection(s: TcpStream, token: &str) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    let mut line = String::new();
    BufReader::new(s.try_clone()?).read_line(&mut line)?;
    let v: Value = serde_json::from_str(&line).unwrap_or_default();
    let (content, err) = if v["token"].as_str() != Some(token) { (vec![text("Material Forge refused the call (wrong token).")], true) } else { call(v["tool"].as_str().unwrap_or_default(), &v["args"]) };
    let mut s = s;
    writeln!(s, "{}", json!({"content": content, "isError": err}))?;
    s.flush()
}

fn repaint() {
    if let Some(c) = lock().ctx.clone() {
        c.request_repaint();
    }
}

/// A call the window answers.
fn on_window(tool: &str, args: &Value) -> Result<Vec<Value>, String> {
    let (tx, rx) = mpsc::channel();
    lock().inbox.push(Cmd { tool: tool.to_string(), args: args.clone(), reply: tx });
    repaint();
    rx.recv_timeout(Duration::from_secs(30)).unwrap_or_else(|_| Err("Material Forge did not respond. If its window is minimized, ask the user to restore it, then try again.".into()))
}

fn call(tool: &str, args: &Value) -> (Vec<Value>, bool) {
    lock().seen = Some(Instant::now());
    match tool {
        "hello" => (Vec::new(), false),
        "texture" => match on_window(tool, args) {
            Ok(_) => outcome(),
            Err(e) => (vec![text(e)], true),
        },
        "answer" => {
            let a = match &args["answer"] {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                v => v.to_string(),
            };
            {
                let mut st = lock();
                if !st.question {
                    return (vec![text("Nothing is waiting for an answer: send the user's request with texture first.")], true);
                }
                if !st.pages.is_empty() {
                    return (vec![text("There is more of Material Forge's request to read first: call wait, then answer.")], true);
                }
                if a.trim().is_empty() {
                    return (vec![text("The answer is empty: send the JSON object the instructions describe.")], true);
                }
                st.answer = Some(a);
                st.question = false;
                link().cv.notify_all();
            }
            repaint();
            outcome()
        }
        "wait" => outcome(),
        "status" | "look" | "open" => match on_window(tool, args) {
            Ok(c) => (c, false),
            Err(e) => (vec![text(e)], true),
        },
        _ => (vec![text(format!("Material Forge has no tool named {tool}."))], true),
    }
}

/// What a request gives back: the next part of a question, the turn's
/// end, or (after `WAIT`) that the work goes on.
fn outcome() -> (Vec<Value>, bool) {
    let t0 = Instant::now();
    let mut st = lock();
    loop {
        if let Some(mut p) = st.pages.pop_front() {
            if st.pages.is_empty() {
                p.push(text("END OF MATERIAL FORGE'S REQUEST. Reply by calling the answer tool with your JSON answer, exactly as the instructions describe (one JSON object, nothing else)."));
            } else {
                p.push(text(format!("(Material Forge's request goes on: {} more part(s). Call wait to read the next one before you answer.)", st.pages.len())));
            }
            return (p, false);
        }
        if let Some(d) = st.done.take() {
            return (d, false);
        }
        if st.question {
            return (vec![text("Material Forge is waiting for your answer to its request: call answer.")], false);
        }
        if !st.turn {
            return (vec![text("Nothing is running in Material Forge now.")], false);
        }
        if t0.elapsed() > WAIT {
            let s = if st.status.is_empty() { "working".to_string() } else { st.status.trim_end_matches("...").to_string() };
            return (vec![text(format!("Material Forge is still on it ({s}). Call wait to get the result."))], false);
        }
        st = link().cv.wait_timeout(st, Duration::from_millis(250)).map(|r| r.0).unwrap_or_else(|e| e.into_inner().0);
    }
}

// ------------------------------------------------------------ the bridge

const SERVER_INSTRUCTIONS: &str = "Material Forge is a 3D texture painter (PBR, like Substance Painter) open on the user's computer. With these tools you texture and light the model open in it, the way its Claude panel does: call texture with the user's request, read Material Forge's request that comes back (its rules on the first message, facts and images of the model), and reply with answer (a JSON object, exactly as its rules say). Material Forge applies it, may ask you to check the result on a render (answer the same way), and ends with Done and a render. Material Forge cannot see images attached to this chat: reference photos go by file path in texture's images (or the user drops them on Material Forge's Claude panel).";

fn tools() -> Value {
    json!([
        {
            "name": "texture",
            "title": "Texture the model",
            "description": "Textures or lights the 3D model open in Material Forge, as the user asks (materials, prints and labels from photos, wear, packshot lighting). It starts a turn of Material Forge's texturing conversation and returns Material Forge's request: its rules (with the first message of a conversation), facts about the model, and images (the model's numbered pieces, the scanned materials, its surfaces, reference photos). Read all of it (when it says more parts follow, call wait), then call answer with your JSON answer. Later messages change only what the user asks for. Pass the user's words as message. Reference photos go as file paths in images: Material Forge cannot see images attached to this chat, so ask the user for the file path, or to drop the photo on Material Forge's Claude panel.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message": {"type": "string", "description": "The user's request, in their words (for example: \"brushed steel body, black rubber grip\", or \"light it like the attached photo\")."},
                    "images": {"type": "array", "items": {"type": "string"}, "description": "Full paths of reference images on this computer, sent with the message (they become R1, R2... of the conversation)."},
                    "texture_sets": {"type": "array", "items": {"type": "string"}, "description": "On a conversation's first message: the texture sets to texture (all when left out)."},
                    "style": {"type": "string", "enum": ["auto", "product", "realistic", "stylized"], "description": "The texturing style (auto: chosen from the object and the request)."},
                    "new_conversation": {"type": "boolean", "description": "Starts a new texturing conversation (the layers already made stay on the model)."},
                    "resend_instructions": {"type": "boolean", "description": "Continue Material Forge's conversation in a chat that has not seen its instructions yet: they come again with this message."}
                },
                "required": ["message"]
            }
        },
        {
            "name": "answer",
            "title": "Answer Material Forge",
            "description": "Your answer to Material Forge's current request: the JSON object its instructions describe, as text. Returns Material Forge's next request (a self check of the result, to answer the same way) or Done with the notes and a render of the result.",
            "inputSchema": {"type": "object", "properties": {"answer": {"type": "string", "description": "One JSON object, exactly as the instructions describe."}}, "required": ["answer"]}
        },
        {
            "name": "wait",
            "title": "Wait for Material Forge",
            "description": "Gets the next part of Material Forge's request, or the result when Material Forge said it is still working.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "look",
            "title": "Look at the model",
            "description": "A render of the model as it looks now in Material Forge: front and back on top, close-ups of its detailed areas below.",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "status",
            "title": "Material Forge status",
            "description": "What is open in Material Forge: the project, its texture sets, the texturing conversation and what it is doing.",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": {"readOnlyHint": true}
        },
        {
            "name": "open",
            "title": "Open a project or a mesh",
            "description": "Opens a Material Forge project (.sprp), or a mesh (.fbx, .obj, .gltf, .glb...) as a new project. Refused while the open project has unsaved changes.",
            "inputSchema": {"type": "object", "properties": {"path": {"type": "string", "description": "Full path of the file."}}, "required": ["path"]}
        }
    ])
}

/// `material_forge --mcp`: the MCP server Claude starts. Never returns.
pub fn bridge() -> ! {
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let send = {
        let out = out.clone();
        move |v: Value| {
            let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
            let _ = writeln!(o, "{v}");
            let _ = o.flush();
        }
    };
    // Hello to the window now and then (its panel shows the link).
    std::thread::spawn(|| loop {
        let _ = to_app("hello", &json!({}), Duration::from_secs(5));
        std::thread::sleep(HELLO);
    });
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let id = v.get("id").cloned();
        let method = v["method"].as_str().unwrap_or_default().to_string();
        let Some(id) = id else { continue }; // notifications
        let reply = |result: Value| json!({"jsonrpc": "2.0", "id": id, "result": result});
        match method.as_str() {
            "initialize" => {
                let asked = v["params"]["protocolVersion"].as_str().unwrap_or_default();
                let version = if ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"].contains(&asked) { asked } else { "2025-06-18" };
                send(reply(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "material-forge", "title": "Material Forge", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": SERVER_INSTRUCTIONS,
                })));
            }
            "ping" => send(reply(json!({}))),
            "tools/list" => send(reply(json!({"tools": tools()}))),
            "tools/call" => {
                let send = send.clone();
                let name = v["params"]["name"].as_str().unwrap_or_default().to_string();
                let args = v["params"]["arguments"].clone();
                let progress = v["params"]["_meta"]["progressToken"].clone();
                let id = id.clone();
                std::thread::spawn(move || {
                    // Progress now and then while the window works (some
                    // clients wait longer then).
                    let done = Arc::new(AtomicBool::new(false));
                    if !progress.is_null() {
                        let (done, send, progress) = (done.clone(), send.clone(), progress.clone());
                        std::thread::spawn(move || {
                            let mut k = 0;
                            while !done.load(Ordering::Relaxed) {
                                std::thread::sleep(Duration::from_secs(1));
                                k += 1;
                                if k % 8 == 0 && !done.load(Ordering::Relaxed) {
                                    send(json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progressToken": progress, "progress": k / 8, "message": "Material Forge is working"}}));
                                }
                            }
                        });
                    }
                    let (content, err) = to_app(&name, &args, Duration::from_secs(180)).unwrap_or_else(|e| (vec![text(e)], true));
                    done.store(true, Ordering::Relaxed);
                    send(json!({"jsonrpc": "2.0", "id": id, "result": {"content": content, "isError": err}}));
                });
            }
            _ => send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("no method {method}")}})),
        }
    }
    std::process::exit(0)
}

/// A call to the window, through its socket.
fn to_app(tool: &str, args: &Value, timeout: Duration) -> Result<(Vec<Value>, bool), String> {
    const OFF: &str = "Material Forge is not open. Ask the user to open Material Forge (with the project to texture), then call this tool again.";
    let info: Value = std::fs::read(info_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or(OFF)?;
    let port = info["port"].as_u64().ok_or(OFF)? as u16;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|_| OFF)?;
    s.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    writeln!(s, "{}", json!({"token": info["token"], "tool": tool, "args": args})).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).map_err(|e| format!("Material Forge did not answer ({e})."))?;
    let v: Value = serde_json::from_str(&line).map_err(|_| "Material Forge's answer could not be read.".to_string())?;
    Ok((v["content"].as_array().cloned().unwrap_or_default(), v["isError"].as_bool().unwrap_or(false)))
}

// ------------------------------------------------- adding it to Claude

/// The Claude desktop app's settings file: the one it uses when it has
/// one (the Store build keeps its own), else where a new one goes.
pub(super) fn claude_app_config() -> PathBuf {
    // Tests (the crawl clicks every button) never touch the real one.
    if super::HEADLESS.load(Ordering::Relaxed) {
        return info_path().with_file_name("claude_desktop_config.json");
    }
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();
    let roaming = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
    let mut found = Vec::new();
    let mut store_dirs = Vec::new();
    for (base, inner) in [(local.join("Claude-Data"), ""), (local.join("Packages"), "LocalCache/Roaming/Claude")] {
        for e in std::fs::read_dir(&base).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.starts_with("claude") {
                let dir = if inner.is_empty() { e.path() } else { e.path().join(inner) };
                if inner.is_empty() {
                    store_dirs.push(dir.clone());
                }
                found.push(dir.join("claude_desktop_config.json"));
            }
        }
    }
    found.push(roaming.join("Claude").join("claude_desktop_config.json"));
    found.iter().find(|p| p.is_file()).cloned().or_else(|| store_dirs.first().map(|d| d.join("claude_desktop_config.json"))).unwrap_or_else(|| roaming.join("Claude").join("claude_desktop_config.json"))
}

/// Where Material Forge is a connector now: the command the Claude app's
/// settings and Claude Code's (user scope) start, read every few seconds.
type Added = (Option<String>, Option<String>);
static ADDED: Mutex<Option<(Instant, Added)>> = Mutex::new(None);

pub(super) fn added() -> Added {
    let mut c = ADDED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, v)) = c.as_ref().filter(|(t, _)| t.elapsed() < Duration::from_secs(3)) {
        return v.clone();
    }
    let read = |p: PathBuf| -> Option<String> {
        let v: Value = serde_json::from_slice(&std::fs::read(p).ok()?).ok()?;
        v["mcpServers"]["material-forge"]["command"].as_str().map(String::from)
    };
    let app = read(claude_app_config());
    let code = if super::HEADLESS.load(Ordering::Relaxed) { None } else { std::env::var_os("USERPROFILE").and_then(|h| read(PathBuf::from(h).join(".claude.json"))) };
    *c = Some((Instant::now(), (app.clone(), code.clone())));
    (app, code)
}

/// Reads the settings again on the next look (after adding).
pub(super) fn forget_added() {
    *ADDED.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Adds (or updates) the Material Forge entry in a Claude settings file,
/// keeping everything else; the old file is kept beside it as .bak.
pub(super) fn add_to_config(path: &Path, exe: &Path) -> Result<(), String> {
    let mut v: Value = match std::fs::read(path) {
        Ok(b) if !b.iter().all(u8::is_ascii_whitespace) => serde_json::from_slice(&b).map_err(|e| format!("{} is not valid JSON ({e}): it was left as it is.", path.display()))?,
        _ => json!({}),
    };
    if !v.is_object() {
        return Err(format!("{} does not hold settings: it was left as it is.", path.display()));
    }
    if !v["mcpServers"].is_object() {
        v["mcpServers"] = json!({});
    }
    v["mcpServers"]["material-forge"] = json!({"command": exe.display().to_string(), "args": ["--mcp"]});
    if path.is_file() {
        std::fs::copy(path, path.with_extension("json.bak")).map_err(|e| format!("No backup of {} ({e}): it was left as it is.", path.display()))?;
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&v).unwrap_or_default()).map_err(|e| format!("{} could not be written ({e}).", path.display()))
}

/// Claude Code's command, when it is installed.
pub(super) fn claude_code() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(home.map(|h| h.join(".local").join("bin")))
        .flat_map(|d| [d.join("claude.exe"), d.join("claude.cmd")])
        .find(|p| p.is_file())
}

/// Adds Material Forge to Claude Code (for every folder: user scope).
pub(super) fn add_to_claude_code(exe: &Path) -> Result<(), String> {
    if super::HEADLESS.load(Ordering::Relaxed) {
        return Err("Not in tests: Claude Code's settings stay as they are.".into());
    }
    let cli = claude_code().ok_or("Claude Code is not installed.")?;
    let run = |args: &[&std::ffi::OsStr]| {
        let mut c = std::process::Command::new(&cli);
        c.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // no console window
        }
        c.output()
    };
    let os = |s: &'static str| std::ffi::OsStr::new(s);
    let _ = run(&[os("mcp"), os("remove"), os("-s"), os("user"), os("material-forge")]);
    let o = run(&[os("mcp"), os("add"), os("-s"), os("user"), os("material-forge"), os("--"), exe.as_os_str(), os("--mcp")]).map_err(|e| format!("Claude Code could not be run ({e})."))?;
    if o.status.success() {
        Ok(())
    } else {
        Err(format!("Claude Code refused: {}", String::from_utf8_lossy(&o.stderr).trim().chars().take(200).collect::<String>()))
    }
}

/// `MF_UITEST_ONLY=studiomuse`: parts of a question, the settings entry
/// (on a scratch file).
pub(super) fn self_check(dir: &Path) -> anyhow::Result<()> {
    let big = "x".repeat(PAGE_TEXT - 10) + "\n" + &"y".repeat(PAGE_TEXT - 10) + "\n";
    let img = image(&image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 8, image::Rgb([1, 2, 3])))).unwrap();
    let pages = paginate(vec![text(big), text("message"), img.clone(), img.clone(), img.clone(), img.clone(), img]);
    let kinds: Vec<&str> = pages.iter().flatten().map(|b| b["type"].as_str().unwrap_or("")).collect();
    anyhow::ensure!(pages.len() == 3 && kinds == ["text", "text", "text", "image", "image", "image", "image", "image"], "parts: {} {kinds:?}", pages.len());
    anyhow::ensure!(pages.iter().all(|p| p.iter().filter(|b| b["type"] == "image").count() <= PAGE_COUNT), "too many images in a part");
    anyhow::ensure!(pages.iter().all(|p| p.iter().filter_map(|b| b["text"].as_str()).map(str::len).sum::<usize>() <= PAGE_TEXT), "a part is too long");
    let cfg = dir.join("claude_desktop_config.json");
    std::fs::write(&cfg, r#"{"mcpServers": {"godot": {"command": "godot.exe"}}, "preferences": {"a": true}}"#)?;
    add_to_config(&cfg, Path::new("C:/MF/material_forge.exe")).map_err(anyhow::Error::msg)?;
    let v: Value = serde_json::from_slice(&std::fs::read(&cfg)?)?;
    anyhow::ensure!(v["mcpServers"]["godot"]["command"] == "godot.exe" && v["preferences"]["a"] == true, "other settings must stay: {v}");
    anyhow::ensure!(v["mcpServers"]["material-forge"]["args"][0] == "--mcp" && v["mcpServers"]["material-forge"]["command"] == "C:/MF/material_forge.exe", "entry: {v}");
    anyhow::ensure!(cfg.with_extension("json.bak").is_file(), "the old settings must be kept");
    std::fs::write(&cfg, "{ broken")?;
    anyhow::ensure!(add_to_config(&cfg, Path::new("x")).is_err() && std::fs::read_to_string(&cfg)? == "{ broken", "a broken file must be left alone");
    println!("MCP parts and Claude settings entry OK");
    Ok(())
}
