"""Служебный режим: статус для wireproxy-control без секретов."""
from __future__ import annotations

import asyncio
import json
import os
import time
from pathlib import Path
from typing import Optional

from . import __version__
from . import upstream
from .config import proxy_config
from .stats import stats

_STARTED_AT = time.time()


def mask_secret(secret: str) -> str:
    if len(secret) <= 8:
        return "***"
    return f"{secret[:4]}…{secret[-4:]}"


def _payload(listening: bool, error: Optional[str]) -> dict:
    now = time.time()
    return {
        "version": __version__,
        "pid": os.getpid(),
        "startedAt": _STARTED_AT,
        "updatedAt": now,
        "uptimeSec": int(now - _STARTED_AT),
        "listening": listening,
        "host": proxy_config.host,
        "port": proxy_config.port,
        "managed": proxy_config.managed,
        "fakeTls": bool(proxy_config.fake_tls_domain),
        "upstream": upstream.snapshot(),
        "stats": {
            "connectionsTotal": stats.connections_total,
            "connectionsActive": stats.connections_active,
            "connectionsWs": stats.connections_ws,
            "connectionsH2": stats.connections_h2,
            "connectionsCf": stats.connections_cfproxy,
            "connectionsTcpFallback": stats.connections_tcp_fallback,
            "connectionsBad": stats.connections_bad,
            "bytesUp": stats.bytes_up,
            "bytesDown": stats.bytes_down,
            "wsErrors": stats.ws_errors,
        },
        "error": error,
    }


def write_status(listening: bool = True, error: Optional[str] = None) -> None:
    path = proxy_config.status_file
    if not path:
        return
    target = Path(path)
    try:
        target.parent.mkdir(parents=True, exist_ok=True)
        tmp = target.with_suffix(target.suffix + ".tmp")
        tmp.write_text(
            json.dumps(_payload(listening, error), ensure_ascii=False),
            encoding="utf-8",
        )
        os.replace(tmp, target)
    except OSError:
        pass


async def status_loop(stop_event: Optional[asyncio.Event] = None) -> None:
    while True:
        write_status(True)
        if stop_event is not None:
            try:
                await asyncio.wait_for(stop_event.wait(), timeout=2.0)
                return
            except asyncio.TimeoutError:
                continue
        await asyncio.sleep(2.0)
