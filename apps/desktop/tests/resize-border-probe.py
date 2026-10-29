"""Read a 41x1 strip through the known E2E border; record only blue positions.
No screenshot, desktop text, or non-border pixel values are saved.
"""
import ctypes,json,sys,threading,time,struct
from ctypes import wintypes as w
u,g=ctypes.windll.user32,ctypes.windll.gdi32
u.SetProcessDpiAwarenessContext.argtypes=[ctypes.c_void_p]
u.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
u.GetDC.argtypes=[w.HWND];u.GetDC.restype=w.HDC
u.ReleaseDC.argtypes=[w.HWND,w.HDC]
g.CreateCompatibleDC.argtypes=[w.HDC];g.CreateCompatibleDC.restype=w.HDC
g.CreateDIBSection.argtypes=[w.HDC,ctypes.c_void_p,w.UINT,ctypes.POINTER(ctypes.c_void_p),w.HANDLE,w.DWORD];g.CreateDIBSection.restype=w.HBITMAP
g.SelectObject.argtypes=[w.HDC,w.HANDLE];g.SelectObject.restype=w.HANDLE
g.BitBlt.argtypes=[w.HDC,ctypes.c_int,ctypes.c_int,ctypes.c_int,ctypes.c_int,w.HDC,ctypes.c_int,ctypes.c_int,w.DWORD]
g.DeleteObject.argtypes=[w.HANDLE];g.DeleteDC.argtypes=[w.HDC]
if sys.argv[1]=='count':
 result=[]
 callback=ctypes.WINFUNCTYPE(w.BOOL,w.HWND,w.LPARAM)
 def inspect(hwnd,_):
  name=ctypes.create_unicode_buffer(128);u.GetClassNameW(hwnd,name,128)
  if name.value.startswith('ScreenTranslateResize'):result.append(int(hwnd))
  return True
 u.EnumWindows(callback(inspect),0);print(len(result));sys.exit()
a=json.loads(sys.argv[1]);width=41;left=round(a['x'])-20;y=round(a['y'])
dc=u.GetDC(None);mem=g.CreateCompatibleDC(dc);bits=ctypes.c_void_p()
info=ctypes.create_string_buffer(struct.pack('<IiiHHIIiiII',40,width,-1,1,32,0,width*4,0,0,0,0)+bytes(16))
bitmap=g.CreateDIBSection(dc,ctypes.byref(info),0,ctypes.byref(bits),None,0);old=g.SelectObject(mem,bitmap)
stop=threading.Event();threading.Thread(target=lambda:(sys.stdin.readline(),stop.set()),daemon=True).start()
rows=[];start=time.perf_counter();print('ready',flush=True)
try:
 while not stop.is_set() and time.perf_counter()-start<15:
  g.BitBlt(mem,0,0,width,1,dc,left,y,0x00CC0020);g.GdiFlush();pixels=ctypes.string_at(bits,width*4)
  blue=[left+i for i in range(width) if pixels[i*4]>230 and 90<pixels[i*4+1]<190 and pixels[i*4+2]<70]
  white=[left+i for i in range(width) if min(pixels[i*4:i*4+3])>245]
  inside=[p for p in white if (p>a['x'] if a.get('inside')=='after' else p<a['x'])]
  gap=min((abs(p-a['x']) for p in inside),default=21)
  rows.append({'atMs':round((time.perf_counter()-start)*1000),'blue':blue,'imageGap':gap});time.sleep(.012)
finally:
 g.SelectObject(mem,old);g.DeleteObject(bitmap);g.DeleteDC(mem);u.ReleaseDC(None,dc)
visible=[r['blue'][0] for r in rows if r['blue']]
print(json.dumps({'rows':rows,'visible':len(visible),'missing':len(rows)-len(visible),'drift':max(visible)-min(visible) if visible else None,'imageMissing':sum(r['imageGap']>4 for r in rows),'maxImageGap':max((r['imageGap'] for r in rows),default=0),'maxWidth':max((len(r['blue']) for r in rows),default=0)}),flush=True)
