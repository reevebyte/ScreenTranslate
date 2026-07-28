"""Small Qt-signal worker backed by a daemon Python thread."""
from __future__ import annotations

from threading import Event, Lock, Thread

from PySide6.QtCore import QObject, Signal


class DaemonWorker(QObject):
    """Run blocking work without making application shutdown depend on it."""

    finished = Signal()

    def __init__(self, *, thread_name: str | None = None) -> None:
        super().__init__()
        self._cancel_event = Event()
        self._thread_lock = Lock()
        self._native_thread: Thread | None = None
        self._thread_name = thread_name or f"ScreenTranslate-{type(self).__name__}"

    @property
    def cancelled(self) -> bool:
        return self._cancel_event.is_set()

    @cancelled.setter
    def cancelled(self, value: bool) -> None:
        if value:
            self._cancel_event.set()
        else:
            self._cancel_event.clear()

    def cancel(self) -> None:
        self._cancel_event.set()

    def is_cancelled(self) -> bool:
        return self._cancel_event.is_set()

    def start(self) -> None:
        with self._thread_lock:
            if self._native_thread is not None:
                raise RuntimeError("worker has already been started")
            thread = Thread(
                target=self._run_wrapper,
                name=self._thread_name,
                daemon=True,
            )
            self._native_thread = thread
            thread.start()

    def isRunning(self) -> bool:
        with self._thread_lock:
            thread = self._native_thread
        return thread is not None and thread.is_alive()

    def wait(self, milliseconds: int = -1) -> bool:
        with self._thread_lock:
            thread = self._native_thread
        if thread is None:
            return True
        timeout = None if milliseconds < 0 else milliseconds / 1000
        thread.join(timeout)
        return not thread.is_alive()

    def _run_wrapper(self) -> None:
        try:
            self.run()
        finally:
            self.finished.emit()

    def run(self) -> None:
        raise NotImplementedError
