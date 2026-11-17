"""Управляемый выбор исходящего пути для tg-ws-proxy.

Порядок путей задаёт менеджер (wireproxy-control): direct, wg, vless.
Для wg/vless используется локальный HTTP CONNECT-прокси wireproxy/xray.
Никаких URL из пользовательского ввода здесь нет: только DC/CF-хосты,
которые уже прошли валидацию в Rust-части.
"""
from __future__ import annotations

import asyncio
import ipaddress
import logging
import socket
from typing import Dict, List, Optional, Tuple

from .config import proxy_config

log = logging.getLogger('tg-mtproto-proxy')

PATH_DIRECT = "direct"
PATH_WG = "wg"
PATH_VLESS = "vless"
VALID_PATHS = (PATH_DIRECT, PATH_WG, PATH_VLESS)

_last_good: Dict[Tuple[str, int], str] = {}
_attempts: Dict[str, int] = {}
_failures: Dict[str, int] = {}
_errors: Dict[str, str] = {}
_last_path = ""


class UpstreamError(RuntimeError):
    pass


def reset() -> None:
    _last_good.clear()
    _attempts.clear()
    _failures.clear()
    _errors.clear()
    global _last_path
    _last_path = ""


def snapshot() -> dict:
    return {
        "last_path": _last_path,
        "attempts": dict(_attempts),
        "failures": dict(_failures),
        "last_errors": dict(_errors),
        "cached_targets": len(_last_good),
    }


def _port(value: str, fallback: int) -> int:
    try:
        port = int(value)
    except (TypeError, ValueError):
        return fallback
    return port if 0 < port < 65536 else fallback


def _proxy_endpoint(path: str) -> Optional[Tuple[str, int]]:
    if path == PATH_WG:
        raw = proxy_config.wg_proxy
    elif path == PATH_VLESS:
        raw = proxy_config.vless_proxy
    else:
        return None
    host, _, port = raw.rpartition(":")
    if not host:
        host, port = raw, "0"
    return host, _port(port, 0)


def _is_public_ip(value: str) -> bool:
    try:
        ip = ipaddress.ip_address(value)
    except ValueError:
        return False
    return ip.is_global and not (
        ip.is_private or ip.is_loopback or ip.is_link_local
        or ip.is_multicast or ip.is_reserved or ip.is_unspecified
    )


def _validate_target(host: str) -> None:
    host = host.strip().lower().rstrip(".")
    if not host or len(host) > 253 or "/" in host or "\\" in host:
        raise UpstreamError("invalid target host")
    if host in ("localhost", "localhost.localdomain") or host.endswith(
        ".localhost"
    ) or host.endswith(".local"):
        raise UpstreamError("refusing local target")
    try:
        ipaddress.ip_address(host)
    except ValueError:
        # Домен: проверяем, что это не имя с недопустимыми символами и что
        # локальный резолв не указывает на приватный/служебный адрес.
        labels = host.split(".")
        if len(labels) < 2:
            raise UpstreamError("invalid target host")
        for label in labels:
            if not label or len(label) > 63:
                raise UpstreamError("invalid target host")
            if label[0] == "-" or label[-1] == "-":
                raise UpstreamError("invalid target host")
            if not all(ch.isalnum() or ch == "-" for ch in label):
                raise UpstreamError("invalid target host")
        try:
            infos = socket.getaddrinfo(host, None, type=socket.SOCK_STREAM)
        except OSError as exc:
            raise UpstreamError(f"cannot resolve target: {exc}") from exc
        addresses = {info[4][0] for info in infos}
        if not addresses or any(not _is_public_ip(addr) for addr in addresses):
            raise UpstreamError("target resolves to a non-public address")
    else:
        if not _is_public_ip(host):
            raise UpstreamError("refusing non-public target")


async def _open_direct(
    host: str,
    port: int,
    ssl_context,
    server_hostname: Optional[str],
    timeout: float,
):
    _validate_target(host)
    kwargs = {}
    if ssl_context is not None:
        kwargs = {"ssl": ssl_context, "server_hostname": server_hostname}
    return await asyncio.wait_for(
        asyncio.open_connection(host, port, **kwargs), timeout=timeout
    )


async def _open_via_proxy(
    path: str,
    host: str,
    port: int,
    ssl_context,
    server_hostname: Optional[str],
    timeout: float,
):
    _validate_target(host)
    endpoint = _proxy_endpoint(path)
    if endpoint is None or endpoint[1] == 0:
        raise UpstreamError(f"{path} proxy is not configured")
    proxy_host, proxy_port = endpoint
    reader, writer = await asyncio.wait_for(
        asyncio.open_connection(proxy_host, proxy_port), timeout=timeout
    )
    try:
        request = (
            f"CONNECT {host}:{port} HTTP/1.1\r\n"
            f"Host: {host}:{port}\r\n"
            "Proxy-Connection: keep-alive\r\n\r\n"
        )
        writer.write(request.encode("ascii"))
        await asyncio.wait_for(writer.drain(), timeout=timeout)
        status_line = await asyncio.wait_for(reader.readline(), timeout=timeout)
        parts = status_line.decode("latin-1", errors="replace").split(" ", 2)
        if len(parts) < 2 or parts[1] != "200":
            raise UpstreamError(
                f"{path} CONNECT rejected: "
                f"{status_line.decode('latin-1', errors='replace').strip()[:80]}"
            )
        while True:
            line = await asyncio.wait_for(reader.readline(), timeout=timeout)
            if line in (b"\r\n", b"\n", b""):
                break
        if ssl_context is not None:
            if not hasattr(writer, "start_tls"):
                raise UpstreamError("Python 3.11+ is required for proxied TLS")
            await asyncio.wait_for(
                writer.start_tls(ssl_context, server_hostname=server_hostname),
                timeout=timeout,
            )
        return reader, writer
    except BaseException:
        writer.close()
        try:
            await writer.wait_closed()
        except Exception:
            pass
        raise


async def open_connection(
    host: str,
    port: int,
    *,
    ssl_context=None,
    server_hostname: Optional[str] = None,
    timeout: float = 10.0,
):
    """Открывает TCP/TLS через первый доступный путь из upstream_order."""
    global _last_path
    order: List[str] = [
        item for item in proxy_config.upstream_order if item in VALID_PATHS
    ] or [PATH_DIRECT]

    key = (host.lower(), port)
    cached = _last_good.get(key)
    if cached in order:
        order = [cached] + [item for item in order if item != cached]

    last_error: Optional[BaseException] = None
    for path in order:
        _attempts[path] = _attempts.get(path, 0) + 1
        try:
            if path == PATH_DIRECT:
                result = await _open_direct(
                    host, port, ssl_context, server_hostname, timeout
                )
            else:
                result = await _open_via_proxy(
                    path, host, port, ssl_context, server_hostname, timeout
                )
            _last_good[key] = path
            _last_path = path
            log.debug("upstream %s -> %s:%d", path, host, port)
            return result
        except BaseException as exc:
            last_error = exc
            _failures[path] = _failures.get(path, 0) + 1
            _errors[path] = str(exc)[:160]
            log.info("upstream %s failed for %s:%d: %r", path, host, port, exc)
            continue

    if last_error is None:
        last_error = UpstreamError("no upstream paths configured")
    raise UpstreamError(str(last_error))
