param([string]$Executable = 'target/release/veloct-desktop.exe', [switch]$RequireBigIcon)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path $Executable).Path
if (Get-Process veloct-desktop -ErrorAction SilentlyContinue) { throw 'Close the running Veloct window before this isolated verification.' }
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class VeloctWindowIconProbe {
 [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
 [DllImport("user32.dll")] static extern bool GetIconInfo(IntPtr h, out ICONINFO i);
 [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr h);
 [DllImport("gdi32.dll")] static extern int GetObjectW(IntPtr h, int n, out BITMAP b);
 [DllImport("gdi32.dll")] static extern int GetDIBits(IntPtr dc, IntPtr bmp, uint start, uint lines, byte[] bits, ref BITMAPINFO info, uint usage);
 [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
 [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr h, IntPtr dc);
 [StructLayout(LayoutKind.Sequential)] struct ICONINFO { public bool icon; public uint x,y; public IntPtr mask,color; }
 [StructLayout(LayoutKind.Sequential)] struct BITMAP { public int type,width,height,widthBytes; public ushort planes,bitsPixel; public IntPtr bits; }
 [StructLayout(LayoutKind.Sequential)] struct BITMAPINFO { public uint size; public int width,height; public ushort planes,bits; public uint compression,imageSize; public int x,y; public uint used,important; public uint color; }
 public static string Pixels(IntPtr icon) {
  if (icon == IntPtr.Zero) return "missing";
  ICONINFO i; if (!GetIconInfo(icon,out i)) throw new Exception("GetIconInfo failed");
  try {
   BITMAP b; if (GetObjectW(i.color,Marshal.SizeOf<BITMAP>(),out b)==0) throw new Exception("GetObject failed");
   var info=new BITMAPINFO{size=40,width=b.width,height=-b.height,planes=1,bits=32};
   var bytes=new byte[b.width*b.height*4]; var dc=GetDC(IntPtr.Zero);
   try { if(GetDIBits(dc,i.color,0,(uint)b.height,bytes,ref info,0)==0) throw new Exception("GetDIBits failed"); } finally { ReleaseDC(IntPtr.Zero,dc); }
   int alpha=0,color=0; for(int p=0;p<bytes.Length;p+=4){if(bytes[p+3]>0)alpha++;if(bytes[p]!=bytes[p+1] || bytes[p+1]!=bytes[p+2])color++;}
   if(alpha==0 || color==0)throw new Exception("Blank/transparent icon");
   return b.width+"x"+b.height+", alpha="+alpha+", chromatic="+color;
  } finally { if(i.color!=IntPtr.Zero)DeleteObject(i.color); if(i.mask!=IntPtr.Zero)DeleteObject(i.mask); }
 }
}
'@
$profile = Join-Path (Split-Path $PSScriptRoot -Parent) ('target/icon-probe-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $profile | Out-Null
$start = [System.Diagnostics.ProcessStartInfo]::new($exe)
$start.UseShellExecute = $false
$start.Environment['APPDATA'] = $profile
$start.Environment['LOCALAPPDATA'] = $profile
# Windows KnownFolder APIs may ignore APPDATA overrides. Force WebView2's own profile too,
# so the icon probe cannot reuse a real localStorage last-project entry and auto-open it.
$start.Environment['WEBVIEW2_USER_DATA_FOLDER'] = (Join-Path $profile 'webview2')
$process = [System.Diagnostics.Process]::Start($start)
try {
 $deadline = [datetime]::UtcNow.AddSeconds(30)
 do { Start-Sleep -Milliseconds 100; $process.Refresh() } while ($process.MainWindowHandle -eq 0 -and !$process.HasExited -and [datetime]::UtcNow -lt $deadline)
 if ($process.MainWindowHandle -eq 0) { throw 'No live main window appeared.' }
 Start-Sleep -Milliseconds 500
 $big = [VeloctWindowIconProbe]::SendMessageW($process.MainWindowHandle, 0x7F, [IntPtr]1, [IntPtr]0)
 $small = [VeloctWindowIconProbe]::SendMessageW($process.MainWindowHandle, 0x7F, [IntPtr]0, [IntPtr]0)
 $result = [ordered]@{path=$exe;sha256=(Get-FileHash $exe).Hash;pid=$process.Id;big=[VeloctWindowIconProbe]::Pixels($big);small=[VeloctWindowIconProbe]::Pixels($small)}
 $result | ConvertTo-Json -Compress
 if ($RequireBigIcon -and $big -eq 0) { throw 'Taskbar ICON_BIG is missing.' }
} finally {
 if (!$process.HasExited) { $null=$process.CloseMainWindow(); if (!$process.WaitForExit(10000)) { Write-Warning 'Test window did not close; close it manually before rebuilding.' } }
 # Keep the isolated profile for evidence; never remove a process-owned profile while it runs.
}
