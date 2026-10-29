"""Sample only five pixels in the E2E fixture's blank white margin.

No screenshots or desktop text are recorded. A black/desktop-colour frame at
either capture handoff increments the nonwhite count. Stop with a stdin line.
"""
import ctypes
import json
import sys
import threading
import time
from ctypes import wintypes

user = ctypes.windll.user32
gdi = ctypes.windll.gdi32
user.SetProcessDpiAwarenessContext.argtypes = [ctypes.c_void_p]
user.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
user.GetDC.argtypes = [wintypes.HWND]
user.GetDC.restype = wintypes.HDC
user.ReleaseDC.argtypes = [wintypes.HWND, wintypes.HDC]
gdi.GetPixel.argtypes = [wintypes.HDC, ctypes.c_int, ctypes.c_int]
gdi.GetPixel.restype = wintypes.DWORD
rect = json.loads(sys.argv[1])
points = [(round(rect['x'] + rect['width'] * t), round(rect['y'])) for t in [0, .25, .5, .75, 1]]
stop = threading.Event()
threading.Thread(target=lambda: (sys.stdin.readline(), stop.set()), daemon=True).start()
dc = user.GetDC(None)
samples = bad = 0
changes = []
start = time.perf_counter()
print('ready', flush=True)
try:
    while not stop.is_set() and time.perf_counter() - start < 30:
        colours = [gdi.GetPixel(dc, x, y) for x, y in points]
        white = all(c != 0xffffffff and min(c & 255, (c >> 8) & 255, (c >> 16) & 255) >= 245 for c in colours)
        samples += 1
        if not white:
            bad += 1
            if len(changes) < 25:
                changes.append({'atMs': round((time.perf_counter() - start) * 1000), 'colours': [f'{c:06x}' for c in colours]})
        time.sleep(.01)
finally:
    user.ReleaseDC(None, dc)
print(json.dumps({'samples': samples, 'nonwhite': bad, 'nonwhiteAtMs': changes, 'durationMs': round((time.perf_counter()-start)*1000)}), flush=True)
