@echo off
rem 双击启动（不弹黑框）。若报错，把 start "" 后面的 pythonw 换成你的 pythonw.exe 全路径。
cd /d "%~dp0"
start "" pythonw "%~dp0run.pyw"
