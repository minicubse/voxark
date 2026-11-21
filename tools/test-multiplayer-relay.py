#!/usr/bin/env python3
"""Fast checks for the UDP fault injector, independent of a game server."""
import importlib.util
from pathlib import Path
import socket
import sys
import unittest

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("multiplayer", Path(__file__).with_name("test-multiplayer.py"))
multiplayer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(multiplayer)


class RelayTests(unittest.TestCase):
    def socket(self):
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.bind(("127.0.0.1", 0))
        sock.settimeout(1)
        self.addCleanup(sock.close)
        return sock

    def test_datagrams_preserved_in_both_directions(self):
        server, client = self.socket(), self.socket()
        relay = multiplayer.UdpRelay(server.getsockname(), 0, 20, 42)
        self.addCleanup(relay.close)
        payload = bytes(range(256)) * 5
        client.sendto(payload, relay.address)
        received, source = server.recvfrom(65535)
        self.assertEqual(received, payload)
        self.assertEqual(source, relay.address)
        server.sendto(payload[::-1], source)
        received, source = client.recvfrom(65535)
        self.assertEqual(received, payload[::-1])
        self.assertEqual(source, relay.address)
        self.assertEqual(relay.dropped, 0)

    def test_full_loss_drops_the_actual_packet(self):
        server, client = self.socket(), self.socket()
        relay = multiplayer.UdpRelay(server.getsockname(), 1, 0, 42)
        self.addCleanup(relay.close)
        server.settimeout(.08)
        client.sendto(b"never delivered", relay.address)
        with self.assertRaises(socket.timeout):
            server.recvfrom(65535)
        self.assertEqual(relay.dropped, 1)
        self.assertEqual(relay.forwarded, 0)


if __name__ == "__main__":
    unittest.main()
