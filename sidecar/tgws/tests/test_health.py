import unittest
from unittest.mock import AsyncMock, patch

from proxy import health
from proxy.config import proxy_config


class _FakeWs:
    async def close(self):
        return None


class HealthMonitorTest(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        health.reset()
        self._old_redirects = proxy_config.dc_redirects
        self._old_media = proxy_config.media_dc_redirects
        self._old_order = proxy_config.upstream_order
        proxy_config.dc_redirects = {2: "149.154.167.220"}
        proxy_config.media_dc_redirects = {}
        proxy_config.upstream_order = []

    def tearDown(self):
        proxy_config.dc_redirects = self._old_redirects
        proxy_config.media_dc_redirects = self._old_media
        proxy_config.upstream_order = self._old_order
        health.reset()

    async def test_failure_then_recovery_invokes_callback_once(self):
        recovered = []
        health.set_recovery_callback(lambda dc, media: recovered.append((dc, media)))

        with patch.object(health.RawWebSocket, "connect", AsyncMock(side_effect=OSError("down"))):
            await health._run_once()
        self.assertFalse(health.snapshot()["dcs"][0]["ok"])

        with patch.object(health.RawWebSocket, "connect", AsyncMock(return_value=_FakeWs())):
            await health._run_once()
        self.assertTrue(health.snapshot()["dcs"][0]["ok"])
        self.assertEqual(recovered, [(2, False)])

        # Staying healthy must not fire the recovery callback again.
        with patch.object(health.RawWebSocket, "connect", AsyncMock(return_value=_FakeWs())):
            await health._run_once()
        self.assertEqual(recovered, [(2, False)])

    async def test_tcp_is_skipped_without_direct_route(self):
        proxy_config.upstream_order = ["wg", "vless"]
        with patch.object(health.RawWebSocket, "connect", AsyncMock(return_value=_FakeWs())):
            await health._run_once()
        snapshot = health.snapshot()
        self.assertEqual(snapshot["tcp"], [])
        self.assertEqual(len(snapshot["dcs"]), 1)


if __name__ == "__main__":
    unittest.main()
