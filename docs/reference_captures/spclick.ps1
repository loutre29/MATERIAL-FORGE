param([int]$X, [int]$Y, [string]$Kind = "left", [string]$Title = "Adobe Substance 3D Painter")
# Posts a click to the window under window-relative capture coordinates (X, Y),
# without moving the real cursor. Kind: left, right, double, move.
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public class PC {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  public struct RECT { public int L, T, R, B; }
  public struct POINT { public int X, Y; }
  public static IntPtr Find(string prefix) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { if (!IsWindowVisible(h)) return true; var sb = new StringBuilder(256); GetWindowText(h, sb, 256);
      if (sb.ToString().StartsWith(prefix)) { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
  }
}
"@
$h = [PC]::Find($Title)
if ($h -eq [IntPtr]::Zero) { "window not found"; exit 1 }
$r = New-Object PC+RECT; [PC]::GetWindowRect($h, [ref]$r) | Out-Null
$p = New-Object PC+POINT; $p.X = $r.L + $X; $p.Y = $r.T + $Y
$target = $h
$c = $p; [PC]::ScreenToClient($target, [ref]$c) | Out-Null
$lp = [IntPtr](($c.Y -shl 16) -bor ($c.X -band 0xFFFF))
$WM_MOUSEMOVE = 0x200; $WM_LBUTTONDOWN = 0x201; $WM_LBUTTONUP = 0x202; $WM_LBUTTONDBLCLK = 0x203; $WM_RBUTTONDOWN = 0x204; $WM_RBUTTONUP = 0x205
[PC]::PostMessage($target, $WM_MOUSEMOVE, [IntPtr]0, $lp) | Out-Null
Start-Sleep -Milliseconds 60
switch ($Kind) {
  "left"   { [PC]::PostMessage($target, $WM_LBUTTONDOWN, [IntPtr]1, $lp) | Out-Null; Start-Sleep -Milliseconds 40; [PC]::PostMessage($target, $WM_LBUTTONUP, [IntPtr]0, $lp) | Out-Null }
  "double" { [PC]::PostMessage($target, $WM_LBUTTONDOWN, [IntPtr]1, $lp) | Out-Null; [PC]::PostMessage($target, $WM_LBUTTONUP, [IntPtr]0, $lp) | Out-Null; [PC]::PostMessage($target, $WM_LBUTTONDBLCLK, [IntPtr]1, $lp) | Out-Null; [PC]::PostMessage($target, $WM_LBUTTONUP, [IntPtr]0, $lp) | Out-Null }
  "right"  { [PC]::PostMessage($target, $WM_RBUTTONDOWN, [IntPtr]2, $lp) | Out-Null; Start-Sleep -Milliseconds 40; [PC]::PostMessage($target, $WM_RBUTTONUP, [IntPtr]0, $lp) | Out-Null }
  default  { }
}
"posted $Kind at client $($c.X),$($c.Y)"
