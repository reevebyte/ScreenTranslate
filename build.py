"""打包成 exe。

    python build.py                  打包成文件夹（启动快，推荐）
    python build.py --onefile        打包成单个 exe（分发方便，启动慢几秒）
    python build.py --with-rapidocr  连 RapidOCR 一起打包（识别艺术字更准，但会大 200MB 左右）

产物在 dist/ 下。
"""
from __future__ import annotations

import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time
from importlib.metadata import PackageNotFoundError, distribution, version
from pathlib import Path

ROOT = Path(__file__).parent.resolve()
ICON = ROOT / "app.ico"
NAME = "ScreenTranslate"
UPDATE_SOURCE_NAME = "screentrans-update-source.json"
_GITHUB_REPOSITORY = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")

BASE_LICENSE_DISTRIBUTIONS = (
    "certifi",
    "charset-normalizer",
    "idna",
    "mss",
    "Pillow",
    "PySide6",
    "PySide6-Addons",
    "PySide6-Essentials",
    "requests",
    "shiboken6",
    "typing_extensions",
    "urllib3",
    "winrt-runtime",
    "winrt-Windows.Foundation",
    "winrt-Windows.Foundation.Collections",
    "winrt-Windows.Globalization",
    "winrt-Windows.Graphics.Imaging",
    "winrt-Windows.Media.Ocr",
    "winrt-Windows.Security.Cryptography",
    "winrt-Windows.Storage.Streams",
    "PyInstaller",
)
RAPIDOCR_LICENSE_DISTRIBUTIONS = (
    "colorama",
    "flatbuffers",
    "numpy",
    "onnxruntime",
    "opencv-python",
    "packaging",
    "protobuf",
    "pyclipper",
    "PyYAML",
    "rapidocr-onnxruntime",
    "Shapely",
    "six",
    "tqdm",
)
_LICENSE_FILE = re.compile(r"^(?:licen[cs]e|copying|notice|authors)(?:[._-].*)?$", re.I)


def build_update_source(environment: dict[str, str] | None = None) -> dict[str, str] | None:
    """Derive the packaged update trust root from GitHub Actions metadata."""
    values = os.environ if environment is None else environment
    repository = str(values.get("GITHUB_REPOSITORY", "") or "").strip()
    if not repository:
        return None
    if _GITHUB_REPOSITORY.fullmatch(repository) is None:
        raise ValueError("GITHUB_REPOSITORY must use OWNER/REPO format")
    owner, name = repository.split("/", 1)
    if owner in (".", "..") or name in (".", ".."):
        raise ValueError("GITHUB_REPOSITORY contains an invalid name")
    channel = str(values.get("SCREENTRANS_UPDATE_CHANNEL", "stable") or "stable").strip()
    if channel not in ("stable", "preview"):
        raise ValueError("SCREENTRANS_UPDATE_CHANNEL must be stable or preview")
    repository_url = f"https://github.com/{owner}/{name}"
    return {
        "manifest_url": f"{repository_url}/releases/latest/download/update-manifest.json",
        "repository_url": repository_url,
        "channel": channel,
    }


def check_build_environment(with_rapid: bool = False) -> bool:
    """Reject environments known to produce an ABI-unsafe executable."""
    try:
        pyside_version = version("PySide6")
    except PackageNotFoundError as exc:
        print(f"[build] 缺少运行依赖：{exc.name}。请先安装 requirements-build.lock")
        return False

    if not with_rapid:
        return True
    try:
        numpy_version = version("numpy")
    except PackageNotFoundError:
        print("[build] RapidOCR 构建缺少 NumPy，请先安装 requirements-rapidocr.lock")
        return False
    try:
        numpy_major = int(numpy_version.split(".", 1)[0])
    except ValueError:
        print(f"[build] 无法识别 NumPy 版本：{numpy_version}")
        return False

    if numpy_major >= 2:
        print(
            "[build] 拒绝打包：当前 NumPy " + numpy_version
            + " 与 PySide6 " + pyside_version + " 的 ABI 不兼容。\n"
            "        请在干净虚拟环境中运行：\n"
            "        python -m pip install -r requirements-build.lock"
        )
        return False
    return True


def copy_license_material(target: Path, with_rapid: bool = False) -> Path:
    """Copy project and exact wheel license material beside the built program."""
    material_root = target if target.is_dir() else target.parent / f"{NAME}-licenses"
    material_root.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / "LICENSE", material_root / "LICENSE.txt")
    shutil.copy2(ROOT / "THIRD_PARTY_NOTICES", material_root / "THIRD_PARTY_NOTICES.txt")

    third_party = material_root / "THIRD_PARTY_LICENSES"
    qt_licenses = third_party / "Qt"
    qt_licenses.mkdir(parents=True, exist_ok=True)
    for source in sorted((ROOT / "licenses").glob("*.txt")):
        shutil.copy2(source, qt_licenses / source.name)

    python_license = next(
        (
            candidate
            for candidate in (Path(sys.base_prefix) / "LICENSE.txt", Path(sys.base_prefix) / "LICENSE")
            if candidate.is_file()
        ),
        None,
    )
    if python_license is not None:
        python_dir = third_party / "CPython"
        python_dir.mkdir(parents=True, exist_ok=True)
        shutil.copy2(python_license, python_dir / python_license.name)

    names = BASE_LICENSE_DISTRIBUTIONS + (
        RAPIDOCR_LICENSE_DISTRIBUTIONS if with_rapid else ()
    )
    for requested_name in names:
        try:
            package = distribution(requested_name)
        except PackageNotFoundError:
            print(f"[build] 许可证提示：没有找到已安装分发包 {requested_name}")
            continue
        package_name = re.sub(
            r"[^A-Za-z0-9_.-]+",
            "-",
            str(package.metadata.get("Name") or requested_name),
        )
        destination_dir = third_party / package_name
        copied_names: set[str] = set()
        for item in package.files or ():
            relative = Path(str(item))
            lower_parts = {part.casefold() for part in relative.parts}
            filename = relative.name
            if (
                "licenses" not in lower_parts
                and _LICENSE_FILE.fullmatch(filename) is None
            ):
                continue
            if "licenseref-qt-commercial" in filename.casefold():
                continue
            source = Path(package.locate_file(item))
            if not source.is_file():
                continue
            output_name = filename
            if output_name.casefold() in copied_names:
                stem, suffix = source.stem, source.suffix
                output_name = f"{stem}-{len(copied_names) + 1}{suffix}"
            copied_names.add(output_name.casefold())
            destination_dir.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination_dir / output_name)
    return material_root

# PySide6 把一整套 Qt 都装进来了，绝大部分用不上。
# 不排掉的话产物会从 ~60MB 涨到 300MB 以上。
EXCLUDE_QT = [
    "PySide6.QtWebEngineCore", "PySide6.QtWebEngineWidgets", "PySide6.QtWebEngineQuick",
    "PySide6.QtQml", "PySide6.QtQuick", "PySide6.QtQuick3D", "PySide6.QtQuickWidgets",
    "PySide6.QtQuickControls2", "PySide6.Qt3DCore", "PySide6.Qt3DRender",
    "PySide6.Qt3DAnimation", "PySide6.Qt3DExtras", "PySide6.Qt3DInput", "PySide6.Qt3DLogic",
    "PySide6.QtMultimedia", "PySide6.QtMultimediaWidgets", "PySide6.QtSpatialAudio",
    "PySide6.QtCharts", "PySide6.QtDataVisualization", "PySide6.QtGraphs",
    "PySide6.QtPdf", "PySide6.QtPdfWidgets", "PySide6.QtSql", "PySide6.QtTest",
    "PySide6.QtDesigner", "PySide6.QtHelp", "PySide6.QtUiTools",
    "PySide6.QtBluetooth", "PySide6.QtNfc", "PySide6.QtPositioning", "PySide6.QtLocation",
    "PySide6.QtSerialPort", "PySide6.QtSerialBus", "PySide6.QtSensors",
    "PySide6.QtTextToSpeech", "PySide6.QtWebSockets", "PySide6.QtWebChannel",
    "PySide6.QtRemoteObjects", "PySide6.QtScxml", "PySide6.QtStateMachine",
    "PySide6.QtHttpServer", "PySide6.QtNetworkAuth", "PySide6.QtOpenGL",
    "PySide6.QtOpenGLWidgets", "PySide6.QtXml", "PySide6.QtDBus",
    "PySide6.QtNetwork", "PySide6.QtSvg",
]
EXCLUDE_OTHER = ["tkinter", "matplotlib", "scipy", "pandas", "IPython", "pytest",
                 "notebook", "PIL.ImageQt", "setuptools", "pip", "winsdk"]

# 不带 RapidOCR 的时候必须把它这一串依赖显式排掉。
# 光靠「不 --collect-all」是不够的：ocr/rapid_ocr.py 里那句
# `from rapidocr_onnxruntime import RapidOCR` 虽然写在函数体内（运行时才执行），
# PyInstaller 静态分析连函数体一起看，照样会把整条链拖进来——
# 实测多出 cv2 95MB + onnxruntime 35MB + shapely，产物从 93MB 涨到 272MB。
# 排掉之后运行时 find_spec 找不到它，设置界面会自动隐藏 RapidOCR 这个选项，不会崩。
EXCLUDE_RAPIDOCR = ["rapidocr_onnxruntime", "onnxruntime", "cv2", "shapely",
                    "onnx", "torch", "torchvision", "skimage", "sklearn"]

# 模块化 WinRT 投影是运行时按名字加载的，PyInstaller 静态分析看不全，
# 必须手工点名。不要换回 winsdk：它把所有命名空间塞进一个 36MB 扩展。
# 改了这里之后一定要跑 `ScreenTranslate.exe --selftest` 确认 OCR 还活着。
WINRT_MODULES = [
    "winrt",
    "winrt.runtime",
    "winrt.system",
    "winrt.windows.foundation",
    "winrt.windows.foundation.collections",
    "winrt.windows.globalization",
    "winrt.windows.graphics.imaging",
    "winrt.windows.media.ocr",
    "winrt.windows.security.cryptography",
    "winrt.windows.storage.streams",
]


def make_icon() -> None:
    """生成多尺寸 ico。轮廓和运行时托盘一致：绿色气泡 + 白色「译」。"""
    from PIL import Image, ImageDraw, ImageFont

    sizes = [16, 20, 24, 32, 48, 64, 128, 256]

    def frame(px: int) -> Image.Image:
        # 4 倍画布抗锯齿。坐标沿用 ui/icon.py 的 64 格轮廓；ICO 生成不能依赖
        # QGuiApplication，所以在 Pillow 里复画同一个连续 silhouette。
        n = px * 4
        unit = n / 64.0
        tiny = px < 22
        mask = Image.new("L", (n, n), 0)
        md = ImageDraw.Draw(mask)

        if tiny:
            md.rounded_rectangle(
                [unit, unit, 63 * unit, 63 * unit],
                radius=14 * unit,
                fill=255,
            )
            body = (unit, unit, 63 * unit, 63 * unit)
        else:
            def quad(start, control, end, steps=12):
                points = []
                for i in range(1, steps + 1):
                    t = i / steps
                    u = 1.0 - t
                    points.append((
                        (u * u * start[0] + 2 * u * t * control[0] + t * t * end[0]) * unit,
                        (u * u * start[1] + 2 * u * t * control[1] + t * t * end[1]) * unit,
                    ))
                return points

            points = [(19 * unit, 2 * unit), (45 * unit, 2 * unit)]
            points += quad((45, 2), (60, 2), (60, 17))
            points.append((60 * unit, 36 * unit))
            points += quad((60, 36), (60, 51), (45, 51))
            points.append((21 * unit, 51 * unit))
            points += quad((21, 51), (10, 54), (4.5, 62))
            points += quad((4.5, 62), (4, 60), (4, 48))
            points.append((4 * unit, 17 * unit))
            points += quad((4, 17), (4, 2), (19, 2))
            md.polygon(points, fill=255)
            body = (4 * unit, 2 * unit, 60 * unit, 51 * unit)

        top = (47, 235, 131)
        bottom = (40, 199, 111)
        layer = Image.new("RGBA", (n, n), (0, 0, 0, 0))
        ld = ImageDraw.Draw(layer)
        for y in range(n):
            t = y / max(1, n - 1)
            color = tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)) + (255,)
            ld.line((0, y, n, y), fill=color)
        layer.putalpha(mask)

        font = None
        font_px = max(9 * 4, round((44 if tiny else 31) * unit))
        for name in ("msyhbd.ttc", "msyh.ttc", "simhei.ttf"):
            try:
                font = ImageFont.truetype(name, font_px)
                break
            except OSError:
                continue
        if font is not None:
            d = ImageDraw.Draw(layer)
            box = d.textbbox((0, 0), "译", font=font)
            cx = (body[0] + body[2]) / 2
            cy = (body[1] + body[3]) / 2 - (0 if tiny else unit)
            d.text(
                (cx - (box[0] + box[2]) / 2, cy - (box[1] + box[3]) / 2),
                "译", font=font, fill=(255, 255, 255, 245),
            )
        return layer.resize((px, px), Image.Resampling.LANCZOS)

    frames = [frame(px) for px in sizes]
    frames[-1].save(
        ICON,
        format="ICO",
        sizes=[(px, px) for px in sizes],
        append_images=frames[:-1],
    )
    print(f"[build] 图标已生成 {ICON}")


# PyInstaller 的 PySide6 钩子会把这些 DLL 一并拷进来，但我们全程用 raster 绘制，
# 没有 OpenGL / QML / PDF，删掉能省约 40MB。删完记得跑 --selftest 复验。
PRUNE = [
    "PySide6/opengl32sw.dll",      # 软件 OpenGL 回退实现，20MB
    "PySide6/Qt6Pdf.dll",
    "PySide6/Qt6Quick.dll",
    "PySide6/Qt6Qml.dll",
    "PySide6/Qt6QmlModels.dll",
    "PySide6/Qt6QmlMeta.dll",
    "PySide6/Qt6QmlWorkerScript.dll",
    "PySide6/Qt6OpenGL.dll",
    "PySide6/Qt6Network.dll",
    "PySide6/Qt6Svg.dll",
    "PySide6/Qt6VirtualKeyboard.dll",
    "PySide6/translations",        # Qt 自带界面的多语言资源，本程序用不到
    "PySide6/plugins/imageformats", # 截图与图标都由 Pillow/内存像素提供
    "PySide6/plugins/iconengines",
    "PySide6/plugins/networkinformation",
    "PySide6/plugins/platforminputcontexts",
    "PySide6/plugins/tls",
    "PySide6/plugins/platforms/qdirect2d.dll",
    "PySide6/plugins/platforms/qminimal.dll",
    "PySide6/plugins/platforms/qoffscreen.dll",
    "PIL/_imagingcms.cp311-win_amd64.pyd",
    "PIL/_webp.cp311-win_amd64.pyd",
]


def _running_pids(exe_name: str) -> list[str]:
    """查一下目标 exe 是不是还开着。开着就别打包了——文件删不掉。"""
    try:
        out = subprocess.run(
            ["tasklist", "/FI", f"IMAGENAME eq {exe_name}", "/NH", "/FO", "CSV"],
            capture_output=True, text=True, timeout=20,
        ).stdout
    except Exception:
        return []
    pids = []
    for line in out.splitlines():
        parts = [p.strip('" ') for p in line.split('","')]
        if len(parts) >= 2 and parts[0].lower() == exe_name.lower() and parts[1].isdigit():
            pids.append(parts[1])
    return pids


def _force_rmtree(path: Path) -> None:
    def _on_error(func, target, _exc):
        # 只读属性会让 rmtree 直接失败，去掉再删一次
        try:
            os.chmod(target, stat.S_IWRITE)
            func(target)
        except OSError:
            pass

    shutil.rmtree(path, onerror=_on_error)


def _clear(path: Path, attempts: int = 12, delay: float = 2.5) -> bool:
    """删掉一个旧产物。删不掉就原样保留，返回 False。

    Windows 上只要目录里有一个文件被占用（程序还开着、资源管理器或终端停在里面、
    杀毒软件正在扫），删除就会中途失败——而 shutil.rmtree(ignore_errors=True)
    会把能删的都删掉再默默返回，留下一个残缺的 dist，比不删还糟。
    所以先整体改名：改名是原子的，成功了才慢慢删，失败了什么都没动。
    """
    if not path.exists():
        return True
    if path.is_file():
        try:
            path.unlink()
            return True
        except OSError:
            return False

    # 只用「改名 + 删改名后的副本」这一条路。
    # 绝不在改名失败后退回直接删：那种删除是部分成功的，
    # 万一旧产物正在被运行（用户开着上一版程序），等于把它的文件一片片拆掉。
    # 刚打包出来的 exe 也常被杀毒软件扫着不放，句柄过几秒才松开，所以要重试。
    for attempt in range(attempts):
        tomb = next(
            (t for t in (path.with_name(f"{path.name}.old{n or ''}") for n in range(20))
             if not t.exists()),
            None,
        )
        if tomb is None:
            print(f"[build] {path.parent} 下堆了太多 .old 残留，请手动清一下")
            return False
        try:
            path.rename(tomb)
        except OSError:
            if attempt < attempts - 1:
                # 杀毒软件扫完几百 MB 的旧产物要一会儿，句柄才会松开
                print(f"[build] {path.name} 被占用，{attempt + 1}/{attempts} 次重试…")
                time.sleep(delay)
            continue
        _force_rmtree(tomb)
        return not path.exists()
    return False


def prune(root: Path) -> None:
    freed = 0
    for rel in PRUNE:
        p = root / rel
        if not p.exists():
            continue
        if p.is_dir():
            freed += sum(f.stat().st_size for f in p.rglob("*") if f.is_file())
            shutil.rmtree(p, ignore_errors=True)
        else:
            freed += p.stat().st_size
            p.unlink()
    print(f"[build] 清理无用组件，省下 {freed / 1024 / 1024:.0f} MB")


def build_subprocess_env() -> dict[str, str]:
    """Keep unrelated developer tools from leaking DLLs into the bundle."""
    env = os.environ.copy()
    windows = Path(env.get("SystemRoot") or env.get("WINDIR") or r"C:\Windows")
    candidates = [
        Path(sys.executable).parent,
        Path(sys.prefix),
        Path(sys.base_prefix),
        Path(sys.base_prefix) / "DLLs",
        windows / "System32",
        windows,
    ]
    clean: list[str] = []
    seen: set[str] = set()
    for candidate in candidates:
        value = str(candidate.resolve())
        key = value.casefold()
        if candidate.exists() and key not in seen:
            clean.append(value)
            seen.add(key)
    env["PATH"] = os.pathsep.join(clean)
    return env


def main() -> int:
    onefile = "--onefile" in sys.argv
    with_rapid = "--with-rapidocr" in sys.argv
    if not check_build_environment(with_rapid):
        return 2
    if with_rapid:
        import importlib.util

        if importlib.util.find_spec("rapidocr_onnxruntime") is None:
            print("[build] 没装 RapidOCR，先运行：pip install rapidocr-onnxruntime")
            return 1

    running = _running_pids(f"{NAME}.exe")
    if running:
        print(
            f"[build] {NAME}.exe 正在运行（PID {', '.join(running)}），先退出它再打包。\n"
            f"        在托盘图标上右键 → 退出。\n"
            f"        （不先关掉的话，旧产物的文件被占用，清理不掉。）"
        )
        return 1

    # 只清理真正要覆盖的东西，不要动 dist 目录本身：
    # 用资源管理器打开着 dist 看产物是很常见的事，那会让 dist 无法删除，
    # 但 PyInstaller 只往 dist/<NAME> 里写，删掉那一层就够了。
    stale_paths = [
        ROOT / "build",
        ROOT / f"{NAME}.spec",
        ROOT / "dist" / NAME,
        ROOT / "dist" / f"{NAME}.exe",
        ROOT / "dist" / f"{NAME}-licenses",
    ]
    for stale in stale_paths:
        if not _clear(stale):
            print(
                f"[build] 无法清理 {stale}\n"
                f"        可能是资源管理器/终端停在该目录里，或杀毒软件正在扫描。\n"
                f"        原产物没有被破坏，关掉占用后重试即可。"
            )
            return 1

    make_icon()

    args = [
        sys.executable, "-m", "PyInstaller",
        "--noconfirm", "--clean",
        "--windowed",                 # 不要控制台黑框
        "--name", NAME,
        "--icon", str(ICON),
        "--collect-binaries", "winrt",
        "--onefile" if onefile else "--onedir",
        str(ROOT / "run.pyw"),
    ]
    for mod in WINRT_MODULES:
        args += ["--hidden-import", mod]
    if with_rapid:
        # 模型文件和 onnxruntime 的原生库都得整包带上
        args += ["--collect-all", "rapidocr_onnxruntime", "--collect-all", "onnxruntime"]
        # 必须用运行时钩子，不能只靠 main.py 里预载：
        # PyInstaller 自动生成的 pyi_rth_pyside6 会在入口脚本之前就 import PySide6，
        # 而 onnxruntime 必须抢在 PySide6 前面加载。自定义钩子排在自动钩子之前。
        args += ["--runtime-hook", str(ROOT / "rthook_onnxruntime.py")]
    excludes = EXCLUDE_QT + EXCLUDE_OTHER + ([] if with_rapid else EXCLUDE_RAPIDOCR + ["numpy"])
    for mod in excludes:
        args += ["--exclude-module", mod]

    try:
        update_source = build_update_source()
    except ValueError as exc:
        print(f"[build] 更新源配置无效：{exc}")
        return 2

    print("[build] 开始打包，需要几分钟…")
    with tempfile.TemporaryDirectory(prefix="screentrans-build-") as temporary:
        if update_source is not None:
            source_path = Path(temporary) / UPDATE_SOURCE_NAME
            source_path.write_text(
                json.dumps(update_source, ensure_ascii=True, indent=2) + "\n",
                encoding="utf-8",
            )
            args += ["--add-data", f"{source_path}{os.pathsep}."]
            print(f"[build] 已嵌入 GitHub 更新源：{update_source['repository_url']}")
        else:
            print("[build] 本地构建未配置 GitHub 更新源")
        result = subprocess.run(args, cwd=ROOT, env=build_subprocess_env())
    if result.returncode != 0:
        print("[build] 打包失败")
        return result.returncode

    target = ROOT / "dist" / (f"{NAME}.exe" if onefile else NAME)
    if not onefile:
        prune(target / "_internal")
    license_root = copy_license_material(target, with_rapid)

    print(f"\n[build] 完成 -> {target}")
    if not onefile:
        print(f"[build] 分发时整个 {target} 文件夹一起拷走，双击里面的 {NAME}.exe 运行。")
    else:
        print(f"[build] 分发时请把 {license_root} 与 EXE 一起提供。")
    print(f"[build] 建议接着跑一次自检：{target}\\{NAME}.exe --selftest"
          if not onefile else f"[build] 建议接着跑一次自检：{target} --selftest")
    # 只统计这一次的产物。以前是把整个 dist 加起来，
    # 里面要是还留着上一次别的产物，报出来的体积就是错的。
    size = sum(f.stat().st_size for f in target.rglob("*") if f.is_file()) \
        if target.is_dir() else target.stat().st_size
    print(f"[build] 体积 {size / 1024 / 1024:.0f} MB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
