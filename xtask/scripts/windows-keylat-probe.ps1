<#
.SYNOPSIS
  Measures keystroke-to-screen latency of puzzle-desktop on Windows (see docs/windows.md).
.DESCRIPTION
  Starts "<Exe> run", focuses its window and alternates '.' and Backspace via SendInput.
  Each sample is the time until the top of the client area changes on screen.
  WARNING: takes focus and types into the window for the whole run (~0.65 s per key).
  The app's stderr goes to $env:TEMP\kl_*.log; its path is left in $global:KLLOG.
.EXAMPLE
  . .\xtask\scripts\windows-keylat-probe.ps1 -Exe .\target\live\puzzle-desktop.exe -N 30 -Backend dx12
#>
param([string]$Exe, [string]$Backend = "", [int]$N = 30)
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System; using System.Diagnostics; using System.Drawing; using System.Runtime.InteropServices; using System.Threading;
public static class KL {
  [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
  [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr h, IntPtr dc);
  [DllImport("gdi32.dll")] static extern uint GetPixel(IntPtr dc, int x, int y);
  [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] i, int s);
  [DllImport("winmm.dll")] static extern uint timeBeginPeriod(uint p);
  [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
  public struct RECT { public int l, t, r, b; } public struct POINT { public int x, y; }
  [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public KI ki; public long pad; }
  [StructLayout(LayoutKind.Sequential)] struct KI { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
  static void Key(ushort vk) {
    var a = new INPUT[2]; a[0].type = 1; a[0].ki.vk = vk; a[1].type = 1; a[1].ki.vk = vk; a[1].ki.flags = 2;
    SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
  }
  static Bitmap bmp; static Graphics g;
  static long Sig(IntPtr dc, int x0, int y0, int w, int h) {
    if (bmp == null) { bmp = new Bitmap(w, h, System.Drawing.Imaging.PixelFormat.Format32bppRgb); g = Graphics.FromImage(bmp); }
    g.CopyFromScreen(x0, y0, 0, 0, new Size(w, h));
    var d = bmp.LockBits(new Rectangle(0, 0, w, h), System.Drawing.Imaging.ImageLockMode.ReadOnly, bmp.PixelFormat);
    var buf = new int[w * h]; Marshal.Copy(d.Scan0, buf, 0, buf.Length); bmp.UnlockBits(d);
    long s = 17; for (int i = 0; i < buf.Length; i++) s = s * 31 + buf[i]; return s;
  }
  public static string Run(IntPtr hwnd, int n) {
    // Physical pixels: without this, scaled displays (e.g. 225%) virtualize rects and capture.
    SetProcessDpiAwarenessContext(new IntPtr(-4)); // PER_MONITOR_AWARE_V2
    timeBeginPeriod(1);
    SetForegroundWindow(hwnd); Thread.Sleep(800);
    RECT r; GetClientRect(hwnd, out r); var p = new POINT(); ClientToScreen(hwnd, ref p);
    // Watch the whole first 6 rows; cmd prompt + echo lives there.
    int w = Math.Min(900, r.r - r.l), h = Math.Min(160, r.b - r.t);
    IntPtr dc = GetDC(IntPtr.Zero);
    var res = new System.Collections.Generic.List<double>();
    for (int i = 0; i < n; i++) {
      ushort vk = (ushort)(i % 2 == 0 ? 0xBE : 0x08); // '.' then Backspace
      long before = Sig(dc, p.x, p.y, w, h);
      var sent = DateTime.UtcNow.ToString("HH:mm:ss.ffffff"); var sw = Stopwatch.StartNew(); Key(vk);
      double ms = -1;
      while (sw.ElapsedMilliseconds < 1000) { if (Sig(dc, p.x, p.y, w, h) != before) { ms = sw.Elapsed.TotalMilliseconds; break; } }
      Console.Error.WriteLine("key " + (vk == 0xBE ? "." : "BS") + " sent=" + sent + " seen_ms=" + ms.ToString("F1")); res.Add(ms); Thread.Sleep(250 + (i * 37) % 100);
    }
    ReleaseDC(IntPtr.Zero, dc);
    var raw = string.Join(" ", res.ConvertAll(v => v.ToString("F0")));
    res.Sort(); var ok = res.FindAll(v => v >= 0);
    if (ok.Count == 0) return "n=0 timeouts=" + res.Count + " (window not captured? check focus/DPI) | raw: " + raw;
    return string.Format("n={0} min={1:F1} p50={2:F1} p90={3:F1} max={4:F1} timeouts={5}", ok.Count, ok[0], ok[ok.Count/2], ok[(int)(ok.Count*0.9)], ok[ok.Count-1], res.Count-ok.Count) + " | raw: " + raw;
  }
}
'@
if ($Backend) { $env:WGPU_BACKEND = $Backend } else { Remove-Item Env:WGPU_BACKEND -ErrorAction SilentlyContinue }
$log = "$env:TEMP\kl_$($Backend)_$(Get-Date -Format HHmmssfff).log"; $global:KLLOG = $log
$p = Start-Process -FilePath $Exe -ArgumentList "run" -RedirectStandardError $log -RedirectStandardOutput "$log.out" -PassThru
try {
  Start-Sleep 4; $p.Refresh()
  "backend=$Backend " + ([KL]::Run($p.MainWindowHandle, $N))
} finally {
  # Always stop the app, or a leftover instance blocks the next link of the exe.
  Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
}
Get-Content $log | Select-String "render.adapter" | ForEach-Object { $_.Line.Substring(28) }
