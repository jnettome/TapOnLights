"""Run asyncio device calls on a dedicated event loop outside the Qt thread."""

from __future__ import annotations

import asyncio
import threading
from concurrent.futures import Future
from typing import Callable, Coroutine, Any

from PySide6.QtCore import QObject, Signal, Slot


class _CompletionBridge(QObject):
    completed = Signal(object, object, object)

    def __init__(self, parent: QObject | None = None) -> None:
        super().__init__(parent)
        self.completed.connect(self._dispatch)

    @Slot(object, object, object)
    def _dispatch(self, callback, value, error) -> None:
        callback(value, error)


class AsyncRunner:
    def __init__(self, parent: QObject | None = None) -> None:
        self.bridge = _CompletionBridge(parent)
        self.loop = asyncio.new_event_loop()
        self.thread = threading.Thread(target=self._run_loop, name="tapo-asyncio", daemon=True)
        self.thread.start()

    def _run_loop(self) -> None:
        asyncio.set_event_loop(self.loop)
        self.loop.run_forever()

    def submit(self, coro: Coroutine[Any, Any, Any], callback: Callable[[Any, Exception | None], None]) -> Future:
        future = asyncio.run_coroutine_threadsafe(coro, self.loop)

        def done(completed: Future) -> None:
            try:
                self.bridge.completed.emit(callback, completed.result(), None)
            except Exception as exc:
                self.bridge.completed.emit(callback, None, exc)

        future.add_done_callback(done)
        return future

    def close(self) -> None:
        async def cancel_pending() -> None:
            current = asyncio.current_task()
            tasks = [task for task in asyncio.all_tasks() if task is not current]
            for task in tasks:
                task.cancel()
            if tasks:
                await asyncio.gather(*tasks, return_exceptions=True)

        try:
            asyncio.run_coroutine_threadsafe(cancel_pending(), self.loop).result(timeout=2)
        except Exception:
            pass
        self.loop.call_soon_threadsafe(self.loop.stop)
        self.thread.join(timeout=2)
        if not self.thread.is_alive():
            self.loop.close()
