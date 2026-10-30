# Puts a short-lived window in the foreground, so the app under test is in the background, as it is
# when someone works in another window during a drill. Fails if it can't take the foreground.
param([switch]$Window)

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Fg {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
}
'@

if ($Window) {
    Add-Type -AssemblyName System.Windows.Forms
    $form = New-Object Windows.Forms.Form -Property @{
        Text = 'auditaur foreground'; Width = 240; Height = 80; TopMost = $true
        StartPosition = 'Manual'; Left = 0; Top = 0; ShowInTaskbar = $false
    }
    $close = New-Object Windows.Forms.Timer -Property @{ Interval = 8000 }
    $close.add_Tick({ $form.Close() })
    $close.Start()
    $form.add_Shown({
        # Windows only lets the process that last had input take the foreground; a synthetic Alt
        # press counts as input.
        [Fg]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
        [Fg]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
        $other = 0
        $thread = [Fg]::GetWindowThreadProcessId([Fg]::GetForegroundWindow(), [ref]$other)
        $mine = [Fg]::GetCurrentThreadId()
        [Fg]::AttachThreadInput($mine, $thread, $true) | Out-Null
        [Fg]::BringWindowToTop($form.Handle) | Out-Null
        [Fg]::SetForegroundWindow($form.Handle) | Out-Null
        [Fg]::AttachThreadInput($mine, $thread, $false) | Out-Null
        $form.Activate()
    })
    [Windows.Forms.Application]::Run($form)
    return
}

# Hide only the console: a hidden Start-Process would also hide the form when it first shows.
$child = Start-Process powershell -PassThru -ArgumentList @(
    '-NoProfile', '-WindowStyle', 'Hidden', '-ExecutionPolicy', 'Bypass', '-File', $PSCommandPath, '-Window')
$deadline = (Get-Date).AddSeconds(5)
while ((Get-Date) -lt $deadline) {
    $owner = 0
    [Fg]::GetWindowThreadProcessId([Fg]::GetForegroundWindow(), [ref]$owner) | Out-Null
    if ($owner -eq $child.Id) {
        'foreground: taken'
        return
    }
    Start-Sleep -Milliseconds 100
}
# Without the foreground the test would pass without testing anything, so fail instead.
$owner = 0
[Fg]::GetWindowThreadProcessId([Fg]::GetForegroundWindow(), [ref]$owner) | Out-Null
[Console]::Error.WriteLine("could not take the foreground from $((Get-Process -Id $owner -EA 0).Name) (pid $owner); is the desktop locked?")
exit 1
