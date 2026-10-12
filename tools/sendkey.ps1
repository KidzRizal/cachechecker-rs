param([int]$VK = 0x74)
# Kirim tombol (default F5) ke window cachechecker-gui via PostMessage (tanpa mencuri fokus).
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class K3 {
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
}
"@
$p = Get-Process cachechecker-gui -ErrorAction Stop | Select-Object -First 1
$h = $p.MainWindowHandle
[K3]::PostMessage($h, 0x100, [IntPtr]$VK, [IntPtr]1) | Out-Null
Start-Sleep -Milliseconds 60
[K3]::PostMessage($h, 0x101, [IntPtr]$VK, [IntPtr]0xC0000001) | Out-Null
"sent vk=$VK"
