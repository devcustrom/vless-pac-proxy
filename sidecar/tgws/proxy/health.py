"""Background reachability checks for Telegram DC routes.

The proxy itself only notices a broken route when a client happens to use it.
This monitor keeps a current picture of every configured DC (normal and media)
and of the raw TCP fallback endpoints, so the UI can show problems before the
next Telegram reconnect and so the WS pool can reset its backoff as soon as a
route recovers.

Checks go through the normal upstream path selection (`RawWebSocket.connect`
uses `upstream.open_connection`), therefore they validate the same route the
proxy will use: direct, WireGuard or VLESS.
"""
from __future__ import annotations

import asyncio
import logging
import time
from typing import Any, Awaitable, Callable, Dict, List, Optional, Tuple

from .config import proxy_config
from .raw_websocket import RawWebSocket
from .utils import DC_DEFAULT_IPS, WS_PATH, ws_domains

log = logging.getLogger('tg-mtproto-proxy')

CHECK_INTERVAL = 45.0
INITIAL_DELAY = 2.0
WS_TIMEOUT = 8.0
TCP_TIMEOUT = 4.0
ERROR_LIMIT = 180

RecoveryCallback = Callable[[int, bool], None]

_state: Dict[str, Any] = {
    "at": None,
    "intervalSec": int(CHECK_INTERVAL),
    "dcs": [],
    "tcp": [],
}
_last_ok: Dict[Tuple[int, bool], bool] = {}
_recovery: Optional[RecoveryCallback] = None


def snapshot() -> dict:
    """Return a JSON-safe copy for the managed status file."""
    return {
        "at": _state.get("at"),
        "intervalSec": _state.get("intervalSec"),
        "dcs": [dict(item) for item in _state.get("dcs", [])],
        "tcp": [dict(item) for item in _state.get("tcp", [])],
    }


def set_recovery_callback(callback: Optional[RecoveryCallback]) -> None:
    global _recovery
    _recovery = callback


def reset() -> None:
    global _state
    _state = {
        "at": None,
        "intervalSec": int(CHECK_INTERVAL),
        "dcs": [],
        "tcp": [],
    }
    _last_ok.clear()


def _short_error(exc: BaseException) -> str:
    text = repr(exc)
    if len(text) > ERROR_LIMIT:
        text = text[: ERROR_LIMIT - 1] + "…"
    return text


def _result(
    *,
    dc: int,
    media: bool,
    ok: bool,
    latency_ms: float,
    target: str,
    domain: Optional[str] = None,
    error: Optional[str] = None,
    checked_at: Optional[float] = None,
    kind: str = "ws",
) -> dict:
    return {
        "kind": kind,
        "dc": dc,
        "media": media,
        "ok": ok,
        "latencyMs": round(latency_ms),
        "target": target,
        "domain": domain,
        "error": error,
        "checkedAt": checked_at or time.time(),
    }


async def _check_ws(
    dc: int,
    media: bool,
    target: str,
    domains: List[str],
) -> dict:
    started = time.monotonic()
    last_error: Optional[str] = None
    for domain in domains:
        # Fronting is the second mode in the pool as well: try normal SNI
        # first, then the masking SNI only if the normal handshake failed.
        for fronted in (False, True):
            try:
                ws = await RawWebSocket.connect(
                    target,
                    domain,
                    timeout=WS_TIMEOUT,
                    path=WS_PATH,
                    sni="sprinthost.ru" if fronted else None,
                    secure=not proxy_config.disable_secure,
                )
                latency = (time.monotonic() - started) * 1000
                try:
                    await ws.close()
                except Exception:
                    pass
                return _result(
                    dc=dc,
                    media=media,
                    ok=True,
                    latency_ms=latency,
                    target=target,
                    domain=domain,
                )
            except asyncio.CancelledError:
                raise
            except Exception as exc:
                last_error = _short_error(exc)
                log.debug(
                    "health: DC%d%s via %s%s failed: %s",
                    dc,
                    " media" if media else "",
                    domain,
                    " (fronted)" if fronted else "",
                    last_error,
                )
    return _result(
        dc=dc,
        media=media,
        ok=False,
        latency_ms=(time.monotonic() - started) * 1000,
        target=target,
        domain=domains[0] if domains else None,
        error=last_error or "no route",
    )


async def _check_tcp(dc: int, ip: str) -> dict:
    started = time.monotonic()
    writer = None
    try:
        _, writer = await asyncio.wait_for(
            asyncio.open_connection(ip, 443),
            timeout=TCP_TIMEOUT,
        )
        latency = (time.monotonic() - started) * 1000
        writer.close()
        try:
            await writer.wait_closed()
        except Exception:
            pass
        return _result(
            dc=dc,
            media=False,
            ok=True,
            latency_ms=latency,
            target=ip,
            kind="tcp",
        )
    except asyncio.CancelledError:
        raise
    except Exception as exc:
        if writer is not None:
            try:
                writer.close()
            except Exception:
                pass
        return _result(
            dc=dc,
            media=False,
            ok=False,
            latency_ms=(time.monotonic() - started) * 1000,
            target=ip,
            error=_short_error(exc),
            kind="tcp",
        )


async def _run_once() -> None:
    jobs: List[Awaitable[dict]] = []
    for dc, target in sorted(proxy_config.dc_redirects.items()):
        jobs.append(_check_ws(dc, False, target, ws_domains(dc, False)))
    for dc, target in sorted(proxy_config.media_dc_redirects.items()):
        jobs.append(_check_ws(dc, True, target, ws_domains(dc, True)))
    if "direct" in proxy_config.upstream_order:
        for dc, ip in sorted(DC_DEFAULT_IPS.items()):
            jobs.append(_check_tcp(dc, ip))

    results = await asyncio.gather(*jobs, return_exceptions=True)
    dcs: List[dict] = []
    tcp: List[dict] = []
    recovered: List[Tuple[int, bool]] = []
    checked_at = time.time()

    for result in results:
        if isinstance(result, BaseException):
            log.debug("health: check crashed: %s", _short_error(result))
            continue
        if result.get("kind") == "ws":
            result["checkedAt"] = checked_at
            dcs.append(result)
            key = (int(result["dc"]), bool(result["media"]))
            previous = _last_ok.get(key)
            _last_ok[key] = bool(result["ok"])
            if result["ok"] and previous is False:
                recovered.append(key)
        else:
            result["checkedAt"] = checked_at
            tcp.append(result)

    _state["at"] = checked_at
    _state["intervalSec"] = int(CHECK_INTERVAL)
    _state["dcs"] = dcs
    _state["tcp"] = tcp

    if recovered:
        for dc, media in recovered:
            log.info(
                "health: DC%d%s recovered",
                dc,
                " media" if media else "",
            )
            if _recovery is not None:
                try:
                    _recovery(dc, media)
                except Exception as exc:
                    log.debug("health: recovery callback failed: %r", exc)


async def health_loop(stop_event: Optional[asyncio.Event] = None) -> None:
    """Runs forever until cancelled (or stop_event is set)."""
    await asyncio.sleep(INITIAL_DELAY)
    while True:
        try:
            await _run_once()
        except asyncio.CancelledError:
            raise
        except Exception as exc:
            log.warning("health: cycle failed: %r", exc)
        if stop_event is not None:
            try:
                await asyncio.wait_for(stop_event.wait(), timeout=CHECK_INTERVAL)
                return
            except asyncio.TimeoutError:
                pass
        else:
            await asyncio.sleep(CHECK_INTERVAL)
