# -*- mode: python ; coding: utf-8 -*-
import certifi
from PyInstaller.utils.hooks import collect_submodules

hiddenimports = (
    collect_submodules("cryptography")
    + collect_submodules("httpx")
    + collect_submodules("httpcore")
    + ["h2", "hpack", "hyperframe"]
)

a = Analysis(
    ["sidecar_entry.py"],
    pathex=["."],
    binaries=[],
    datas=[(certifi.where(), "certifi")],
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=["tkinter", "customtkinter", "pystray", "PIL", "pyperclip", "psutil"],
    noarchive=False,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name="tgws-proxy",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)
