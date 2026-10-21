param([string]$Title, [string]$Out)
# PrintWindow capture of the first visible window whose title starts with $Title.
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public class Q {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  public struct RECT { public int L, T, R, B; }
  public static IntPtr Find(string prefix) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { if (!IsWindowVisible(h)) return true; var sb = new StringBuilder(256); GetWindowText(h, sb, 256);
      if (sb.ToString().StartsWith(prefix)) { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
  }
}
"@
Add-Type -AssemblyName System.Drawing
$h = [Q]::Find($Title)
if ($h -eq [IntPtr]::Zero) { "window not found"; exit 1 }
$r = New-Object Q+RECT; [Q]::GetWindowRect($h, [ref]$r) | Out-Null
$bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc(); [Q]::PrintWindow($h, $hdc, 2) | Out-Null; $g.ReleaseHdc($hdc)
$bmp.Save($Out); "saved $Out ($($bmp.Width)x$($bmp.Height))"
