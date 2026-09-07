#!/usr/bin/env python3
"""Run under dbus-run-session -- xvfb-run -a python3 tests/desktop_smoke.py BINARY.

Requires python3-dbus, python3-gi, Xvfb and xdotool. Uses an isolated configuration
and local ICS server; never connects to a real calendar or keyring.
"""
import datetime
import http.server
import os
import shutil
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time

import dbus
import dbus.mainloop.glib
import dbus.service
from gi.repository import GLib

WATCHER = "org.kde.StatusNotifierWatcher"
PROPERTIES = "org.freedesktop.DBus.Properties"
ITEM = "org.kde.StatusNotifierItem"
MENU = "com.canonical.dbusmenu"


def pump_until(predicate, description, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        while GLib.MainContext.default().iteration(False):
            pass
        result = predicate()
        if result:
            return result
        time.sleep(0.02)
    raise AssertionError("Timed out: " + description)


class Watcher(dbus.service.Object):
    def __init__(self, bus):
        self.name = dbus.service.BusName(WATCHER, bus=bus)
        super().__init__(bus, "/StatusNotifierWatcher")
        self.items = []

    @dbus.service.method(WATCHER, in_signature="s", out_signature="", sender_keyword="sender")
    def RegisterStatusNotifierItem(self, service, sender=None):
        self.items.append((sender if service.startswith("/") else service,
                           service if service.startswith("/") else "/StatusNotifierItem"))
        self.StatusNotifierItemRegistered(service)

    @dbus.service.signal(WATCHER, signature="s")
    def StatusNotifierItemRegistered(self, service):
        pass

    @dbus.service.method(PROPERTIES, in_signature="ss", out_signature="v")
    def Get(self, interface, name):
        return self.GetAll(interface)[name]

    @dbus.service.method(PROPERTIES, in_signature="s", out_signature="a{sv}")
    def GetAll(self, interface):
        assert interface == WATCHER
        return {"IsStatusNotifierHostRegistered": dbus.Boolean(True),
                "ProtocolVersion": dbus.Int32(0),
                "RegisteredStatusNotifierItems": dbus.Array([], signature="s")}


class Calendar(http.server.BaseHTTPRequestHandler):
    fail = False

    def do_GET(self):
        if self.fail:
            self.send_error(503, "Smoke-test calendar temporarily unavailable")
            return
        day = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%d")
        body = ("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Meeters//Smoke//EN\r\n"
                "BEGIN:VEVENT\r\nUID:smoke-test\r\n"
                f"DTSTART:{day}T090000Z\r\nDTEND:{day}T093000Z\r\n"
                "SUMMARY:Migration smoke meeting\r\n"
                "DESCRIPTION:https://example.zoom.us/j/123456789\r\n"
                "END:VEVENT\r\nBEGIN:VEVENT\r\nUID:smoke-all-day\r\n"
                f"DTSTART;VALUE=DATE:{day}\r\n"
                "SUMMARY:All-day migration check\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n").encode()
        self.send_response(200)
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass


def main():
    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus()
    watcher = Watcher(bus)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Calendar)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix="meeters-smoke-") as tmp:
        env = {k: v for k, v in os.environ.items() if not k.startswith("MEETERS_")}
        env.update(XDG_CONFIG_HOME=tmp, GDK_BACKEND="x11", GTK_A11Y="none", GSK_RENDERER="cairo", NO_AT_BRIDGE="1", GIO_USE_VFS="local",
                   MEETERS_CALENDAR_SOURCE="ics", MEETERS_LOCAL_TIMEZONE="UTC",
                   MEETERS_ICAL_URL=f"http://127.0.0.1:{server.server_port}/calendar.ics",
                   MEETERS_EVENT_NOTIFICATION="false", MEETERS_POLLING_INTERVAL_MS="1000", MEETERS_LOG="debug")
        config = Path(tmp)
        icons = config / "meeters"
        icons.mkdir()
        for icon in (Path(__file__).resolve().parents[1] / "assets").glob("*.png"):
            shutil.copy(icon, icons)
        # Capture URI launches locally instead of opening a real browser.
        data = config / "data"
        applications = data / "applications"
        applications.mkdir(parents=True)
        launched_uri = config / "launched-uri"
        handler = config / "capture_uri.py"
        handler.write_text("import pathlib,sys\npathlib.Path(" + repr(str(launched_uri)) + ").write_text(sys.argv[1])\n")
        (applications / "meeters-smoke.desktop").write_text(
            "[Desktop Entry]\nType=Application\nName=Smoke URI handler\n"
            f"Exec={sys.executable} {handler} %u\nMimeType=x-scheme-handler/https;\n")
        (config / "mimeapps.list").write_text(
            "[Default Applications]\nx-scheme-handler/https=meeters-smoke.desktop\n")
        env["XDG_DATA_HOME"] = str(data)
        log_path = Path(tmp) / "app.log"
        with log_path.open("w") as log:
            app = subprocess.Popen([str(Path(sys.argv[1]).resolve())], env=env,
                                   stdout=log, stderr=subprocess.STDOUT)
            try:
                pump_until(lambda: watcher.items, "tray registration")
                service, path = watcher.items[0]
                item = bus.get_object(service, path)
                props = dbus.Interface(item, PROPERTIES).GetAll(ITEM)
                menu = dbus.Interface(bus.get_object(service, props["Menu"]), MENU)

                def entries():
                    _, root = menu.GetLayout(0, -1, dbus.Array([], signature="s"))
                    return [(int(child[0]), dict(child[1])) for child in root[2]]

                pump_until(lambda: any("Migration smoke meeting" in p.get("label", "")
                                      for _, p in entries()), "calendar in tray menu")

                def click(label):
                    entry = next(i for i, p in entries() if label in p.get("label", ""))
                    menu.Event(entry, "clicked", dbus.Int32(0, variant_level=1), dbus.UInt32(0))

                def visible(title):
                    found = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", title],
                                           capture_output=True, text=True)
                    return found.stdout.strip()

                click("Migration smoke meeting")
                pump_until(launched_uri.exists, "meeting URI dispatched to desktop handler")
                assert launched_uri.read_text() == "https://example.zoom.us/j/123456789"
                props = dbus.Interface(item, PROPERTIES).GetAll(ITEM)
                assert str(props["IconName"]).startswith("meeters-appindicator")
                assert str(props["IconThemePath"]) == str(icons)
                click("Show Meetings Window")
                window_id = pump_until(lambda: visible("Calendar View"), "calendar window from tray")
                if screenshot := os.environ.get("MEETERS_SMOKE_SCREENSHOT"):
                    subprocess.run(["xdotool", "windowsize", window_id.splitlines()[0], "1450", "1160"], check=True)
                    time.sleep(1)
                    subprocess.run(["import", "-window", window_id.splitlines()[0], screenshot], check=True)
                control = dbus.Interface(bus.get_object("net.aggregat4.Meeters", "/net/aggregat4/Meeters"),
                                         "net.aggregat4.Meeters")
                control.CloseWindow()
                pump_until(lambda: not visible("Calendar View"), "hide via D-Bus")
                assert app.poll() is None, "App exited when calendar was hidden"
                control.ToggleWindow()
                pump_until(lambda: visible("Calendar View"), "toggle via D-Bus")
                control.CloseWindow()
                control.ShowWindow()
                pump_until(lambda: visible("Calendar View"), "show via D-Bus")
                assert visible("Calendar View") == window_id, "Reopening created a duplicate window"
                control.CloseWindow()
                second = subprocess.Popen([str(Path(sys.argv[1]).resolve())], env=env,
                                          stdout=log, stderr=subprocess.STDOUT)
                pump_until(lambda: second.poll() is not None, "second instance forwards activation")
                assert second.returncode == 0
                pump_until(lambda: visible("Calendar View"), "second launch presents existing window")
                assert len(watcher.items) == 1, "Second launch registered another tray"
                status = next(p["label"] for _, p in entries() if p.get("label", "").startswith("Source:"))
                click(status)
                pump_until(lambda: visible("Calendar Refresh Log"), "refresh log from tray")
                Calendar.fail = True
                pump_until(lambda: dbus.Interface(item, PROPERTIES).Get(ITEM, "IconName")
                           == "meeters-appindicator-error", "error tray icon")
                assert any("Migration smoke meeting" in p.get("label", "") for _, p in entries()), "Refresh failure lost meetings"
                Calendar.fail = False
                pump_until(lambda: dbus.Interface(item, PROPERTIES).Get(ITEM, "IconName")
                           != "meeters-appindicator-error", "recovered tray icon")
                click("Quit")
                pump_until(lambda: app.poll() is not None, "quit from tray")
                assert app.returncode == 0, log_path.read_text()
                output = log_path.read_text()
                assert "CRITICAL" not in output, output
                print("PASS: tray registration, meeting links, calendar menu, window actions, D-Bus, single instance, refresh log, error/recovery icons, quit")
            except BaseException:
                print(log_path.read_text(), file=sys.stderr)
                raise
            finally:
                if app.poll() is None:
                    app.terminate()
                    app.wait(timeout=5)
        server.shutdown()


if __name__ == "__main__":
    main()
