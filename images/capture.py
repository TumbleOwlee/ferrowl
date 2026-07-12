#!/usr/bin/env python3
"""Regenerate the README impression capture in `images/`.

Drives the real `ferrowl --demo` binary inside a detached tmux pane, sends the
keystrokes that open each view/dialog, captures the pane with colors
(`tmux capture-pane -e`), and renders the captured ANSI text to a PNG with
Pillow. Fully headless — no real terminal or display needed.

Usage:
    python3 images/capture.py            # all shots into images/
    python3 images/capture.py --only ocpp/action --out /tmp/shots
    python3 images/capture.py --list

Requirements: tmux, Pillow, DejaVu Sans Mono (fonts/ paths below).
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

REPO = Path(__file__).resolve().parent.parent
BINARY = REPO / "target" / "debug" / "ferrowl"
SESSION = "ferrowl-screenshot"

# Terminal grid matching the historical 1181x650 captures (~118x32 cells).
COLS, ROWS = 180, 90

FONT_REGULAR = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
FONT_BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"
FONT_SIZE = 15

# Give the demo modules time to bind/connect before typing (OCPP demo tabs
# dial each other on startup).
STARTUP_DELAY = 2.0
# Default pause after each key chunk so the 100ms UI tick redraws.
KEY_DELAY = 0.4


@dataclass
class Shot:
    """One screenshot: where it goes and how to get the UI there."""

    out: str
    desc: str
    # Each entry is (tmux send-keys arguments, delay-after-seconds).
    keys: list[tuple[list[str], float]] = field(default_factory=list)


def tab(n: int) -> list[tuple[list[str], float]]:
    """Key chunks switching to demo tab `n` via the Ctrl+t digit chord.

    The chord waits 800ms for a second digit before jumping, so the capture
    delay after the digit must exceed it.
    """
    return [(["C-t"], 0.2), ([str(n)], 1.2)]


def ocpp_traffic() -> list[tuple[list[str], float]]:
    """Key chunks that make the demo OCPP pair look alive before a capture.

    On the CS v1.6 tab: send BootNotification (CS scope), then StatusNotification and
    MeterValues (connector scope). Selection lists keep their index across re-syncs, so
    every walk first anchors at the top with a surplus of Ups. Leaves the view back on
    the connector table with the CS row selected.
    """
    return (
        tab(3)
        + [
            (["Enter"], 0.3),                    # sync CS-scope actions
            (["Tab", "Tab", "Tab"], 0.3),        # -> actions pane
            (["select", "BootNotification"], 0),
            (["Enter"], 1.0),                    # send
            (["BTab", "BTab", "BTab"], 0.3),     # -> connector table
            (["g"], 0.2),
            (["j"], 0.2),
            (["Enter"], 0.3),                    # sync connector-scope actions
            (["Tab", "Tab", "Tab"], 0.3),        # -> actions pane
            (["select", "StatusNotification"], 0),
            (["Enter"], 1.0),                    # send
            (["select", "MeterValues"], 0),
            (["Enter"], 1.0),                    # send
            (["BTab", "BTab", "BTab"], 0.3),     # tidy: back to connector table
            (["g"], 0.3),                        # CS row selected
        ]
    )


# Demo tab order: 0 Modbus Server, 1 Modbus Client, 2 CSMS 1.6, 3 CS 1.6,
# 4 CSMS 2.0.1, 5 CS 2.0.1, 6 CSMS 2.1, 7 CS 2.1.
SHOTS = [
    Shot(
        out="command-help.png",
        desc="Modbus server tab with the ':' command popup open",
        keys=[([":"], 0.6)],
    ),
    Shot(
        out="new-module.png",
        desc="Module type selection (:new)",
        keys=[([":new", "Enter"], 0.6)],
    ),
    Shot(
        out="session.png",
        desc="Session dialog (:session) with the demo power-report script and live log",
        # The demo session script runs on a 1s interval, so waiting lets the
        # log pane at the bottom fill with a few report lines.
        keys=[([":session", "Enter"], 3.0)],
    ),
    Shot(
        out="modbus/new-module.png",
        desc="Modbus module setup dialog",
        keys=[([":new", "Enter"], 0.6), (["Enter"], 0.6)],
    ),
    Shot(
        out="modbus/add-register.png",
        desc="Add-register dialog",
        keys=[([":add", "Enter"], 0.6)],
    ),
    Shot(
        out="modbus/edit-register.png",
        desc="Edit dialog for a plain register",
        keys=[(["Enter"], 0.6)],
    ),
    Shot(
        out="modbus/edit-selection-register.png",
        desc="Edit dialog for a register with named values (Mode)",
        # Demo table rows are name-sorted: Current L1..L3, Mode, Power, SetPoint.
        keys=[(["j", "j", "j"], 0.4), (["Enter"], 0.6)],
    ),
    Shot(
        out="ocpp/new-module.png",
        desc="OCPP module setup dialog",
        keys=[([":new", "Enter"], 0.6), (["j"], 0.3), (["Enter"], 0.6)],
    ),
    Shot(
        out="ocpp/client.png",
        desc="OCPP charging-station (client) view",
        keys=ocpp_traffic(),
    ),
    Shot(
        out="ocpp/client-cp.png",
        desc="Client view: CS-scope state (CS row selected)",
        keys=ocpp_traffic() + [(["Enter"], 0.6)],
    ),
    Shot(
        out="ocpp/client-con.png",
        desc="Client view: connector-scope state (connector row selected)",
        keys=ocpp_traffic() + [(["j"], 0.3), (["Enter"], 0.6)],
    ),
    Shot(
        out="ocpp/action.png",
        desc="Client view: actions pane, Authorize sent (messages + payload filled)",
        # Enter on the connector table syncs the action list for the CS scope, three Tabs land
        # on the actions pane, Enter triggers the selected action (v1.6 sends with defaults).
        keys=ocpp_traffic()
        + [
            (["Enter"], 0.3),
            (["Tab", "Tab", "Tab"], 0.3),
            (["select", "Authorize"], 0),
            (["Enter"], 1.0),
        ],
    ),
    Shot(
        out="ocpp/server.png",
        desc="OCPP CSMS (server) view with the demo client connected",
        keys=ocpp_traffic() + tab(2),
    ),
    Shot(
        out="ocpp/server-cp.png",
        desc="Server view: charge-point detail dialog",
        keys=ocpp_traffic() + tab(2) + [(["Enter"], 0.6)],
    ),
    Shot(
        out="ocpp/server-con.png",
        desc="Server view: connector detail dialog",
        keys=ocpp_traffic() + tab(2) + [(["j"], 0.3), (["Enter"], 0.6)],
    ),
]


# --- tmux driving ---------------------------------------------------------


def sh(*args: str, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(args, check=check, capture_output=True, text=True)


def tmux(*args: str, check: bool = True) -> subprocess.CompletedProcess:
    """tmux on a dedicated server socket (-L). On the default socket an already
    running server (e.g. the user's own tmux) would ignore both our -f config
    and the -x/-y size, capturing a wrong-sized pane."""
    return sh("tmux", "-L", SESSION, *args, check=check)


# The focused-selection background of the vscode_dark scheme, as emitted in
# `capture-pane -e` output — used to locate the highlighted list row.
SELECTED_BG = "48;2;86;156;214"


def selected_text(ansi: str) -> str:
    """The first word of the highlighted (focused-selection) row, skipping the tab bar."""
    for line in ansi.splitlines():
        if SELECTED_BG not in line:
            continue
        t = re.sub(r"\x1b\[[0-9;]*m", "", line)
        t = re.sub(r"[│┌┐└┘─╔╗╚╝║]", " ", t).strip()
        if not t or t.startswith("["):  # the selected tab shares the highlight bg
            continue
        return t.split()[0]
    return ""


def pane_ansi() -> str:
    return tmux("capture-pane", "-e", "-N", "-p", "-t", SESSION).stdout


def select_action(target: str) -> None:
    """Closed-loop selection: anchor at the list top, then step down until the
    highlighted row is `target`. Immune to individually swallowed keys, which
    open-loop Up/Down counting proved not to be."""
    for _ in range(10):  # to the top: stop when Up no longer changes the highlight
        before = selected_text(pane_ansi())
        tmux("send-keys", "-t", SESSION, "Up")
        time.sleep(0.15)
        if selected_text(pane_ansi()) == before:
            break
    for _ in range(15):  # down to the target
        if selected_text(pane_ansi()) == target:
            return
        tmux("send-keys", "-t", SESSION, "Down")
        time.sleep(0.15)
    raise RuntimeError(f"could not select action '{target}' (at '{selected_text(pane_ansi())}')")


def capture_shot(shot: Shot) -> str:
    """Run the demo, walk to the shot's UI state, return the ANSI capture."""
    tmux("kill-server", check=False)
    # Truecolor must be configured before the binary starts (crossterm sniffs
    # TERM/COLORTERM at startup and silently degrades RGB otherwise), so the
    # options go into a config file the fresh tmux server reads at launch.
    conf = Path("/tmp") / f"{SESSION}.conf"
    conf.write_text(
        'set -g default-terminal "tmux-256color"\n'
        'set -ga terminal-features ",*:RGB"\n'
        'set -ga terminal-overrides ",*:Tc"\n'
    )
    tmux(
        "-f", str(conf),
        "new-session", "-d",
        "-s", SESSION,
        "-x", str(COLS),
        "-y", str(ROWS),
        "env", "COLORTERM=truecolor", str(BINARY), "--demo",
    )
    try:
        time.sleep(STARTUP_DELAY)
        for keys, delay in shot.keys:
            if keys and keys[0] == "select":
                # Closed-loop list navigation; `delay` is unused for this step.
                select_action(keys[1])
                continue
            # One key per send-keys call: bursts proved lossy against the app's
            # 100ms input tick (keys were swallowed mid-sequence).
            for key in keys:
                tmux("send-keys", "-t", SESSION, key)
                time.sleep(0.15)
            time.sleep(delay if delay else KEY_DELAY)
        # -N preserves trailing spaces: styled padding (e.g. the full-width green
        # status line) is trailing whitespace and would be trimmed without it.
        out = tmux("capture-pane", "-e", "-N", "-p", "-t", SESSION)
        return out.stdout
    finally:
        tmux("kill-server", check=False)


# --- ANSI -> PNG rendering -------------------------------------------------

DEFAULT_FG = (200, 200, 200)
DEFAULT_BG = (12, 12, 12)

BASE16 = [
    (0, 0, 0), (205, 49, 49), (13, 188, 121), (229, 229, 16),
    (36, 114, 200), (188, 63, 188), (17, 168, 205), (229, 229, 229),
    (102, 102, 102), (241, 76, 76), (35, 209, 139), (245, 245, 67),
    (59, 142, 234), (214, 112, 214), (41, 184, 219), (255, 255, 255),
]


def xterm256(n: int) -> tuple[int, int, int]:
    if n < 16:
        return BASE16[n]
    if n < 232:
        n -= 16
        steps = [0, 95, 135, 175, 215, 255]
        return (steps[n // 36], steps[(n // 6) % 6], steps[n % 6])
    v = 8 + (n - 232) * 10
    return (v, v, v)


@dataclass
class Cell:
    ch: str = " "
    fg: tuple = DEFAULT_FG
    bg: tuple = DEFAULT_BG
    bold: bool = False


SGR_RE = re.compile(r"\x1b\[([0-9;:]*)m")
OTHER_ESC_RE = re.compile(r"\x1b(?:\][^\x07]*\x07|\[[0-9;?]*[A-Za-ln-z]|[()][B0])")


def parse_ansi(text: str) -> list[list[Cell]]:
    """Parse `tmux capture-pane -e` output into a COLS x ROWS cell grid."""
    grid = [[Cell() for _ in range(COLS)] for _ in range(ROWS)]
    fg, bg, bold, reverse = DEFAULT_FG, DEFAULT_BG, False, False

    lines = text.split("\n")
    for row, line in enumerate(lines[:ROWS]):
        line = OTHER_ESC_RE.sub("", line)
        col = 0
        i = 0
        while i < len(line) and col < COLS:
            m = SGR_RE.match(line, i)
            if m:
                params = [int(p) if p else 0 for p in re.split("[;:]", m.group(1))] or [0]
                j = 0
                while j < len(params):
                    p = params[j]
                    if p == 0:
                        fg, bg, bold, reverse = DEFAULT_FG, DEFAULT_BG, False, False
                    elif p == 1:
                        bold = True
                    elif p in (21, 22):
                        bold = False
                    elif p == 7:
                        reverse = True
                    elif p == 27:
                        reverse = False
                    elif 30 <= p <= 37:
                        fg = BASE16[p - 30]
                    elif p == 39:
                        fg = DEFAULT_FG
                    elif 40 <= p <= 47:
                        bg = BASE16[p - 40]
                    elif p == 49:
                        bg = DEFAULT_BG
                    elif 90 <= p <= 97:
                        fg = BASE16[p - 90 + 8]
                    elif 100 <= p <= 107:
                        bg = BASE16[p - 100 + 8]
                    elif p in (38, 48) and j + 1 < len(params):
                        mode = params[j + 1]
                        color = None
                        if mode == 5 and j + 2 < len(params):
                            color = xterm256(params[j + 2])
                            j += 2
                        elif mode == 2 and j + 4 < len(params):
                            color = tuple(params[j + 2 : j + 5])
                            j += 4
                        if color is not None:
                            if p == 38:
                                fg = color
                            else:
                                bg = color
                    j += 1
                i = m.end()
                continue
            ch = line[i]
            if ch not in ("\r", "\x1b"):
                cell = grid[row][col]
                cell.ch = ch
                cell.fg, cell.bg = (bg, fg) if reverse else (fg, bg)
                cell.bold = bold
                col += 1
            i += 1
    return grid


def render_png(grid: list[list[Cell]], out: Path) -> None:
    regular = ImageFont.truetype(FONT_REGULAR, FONT_SIZE)
    bold = ImageFont.truetype(FONT_BOLD, FONT_SIZE)
    bbox = regular.getbbox("M")
    cw = bbox[2] - bbox[0]
    ascent, descent = regular.getmetrics()
    ch_h = ascent + descent

    img = Image.new("RGB", (COLS * cw, ROWS * ch_h), DEFAULT_BG)
    draw = ImageDraw.Draw(img)
    for r, row in enumerate(grid):
        for c, cell in enumerate(row):
            x, y = c * cw, r * ch_h
            if cell.bg != DEFAULT_BG:
                draw.rectangle([x, y, x + cw - 1, y + ch_h - 1], fill=cell.bg)
            if cell.ch != " ":
                draw.text((x, y), cell.ch, font=bold if cell.bold else regular, fill=cell.fg)
    out.parent.mkdir(parents=True, exist_ok=True)
    img.save(out)


# --- main ------------------------------------------------------------------


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--only", action="append", help="shot name substring filter (repeatable)")
    ap.add_argument("--out", default=str(REPO / "images"), help="output directory (default: images/)")
    ap.add_argument("--list", action="store_true", help="list shot names and exit")
    args = ap.parse_args()

    if args.list:
        for s in SHOTS:
            print(f"{s.out:40} {s.desc}")
        return 0

    shots = [
        s for s in SHOTS
        if not args.only or any(f in s.out for f in args.only)
    ]
    if not shots:
        print("no shots match --only filter", file=sys.stderr)
        return 2

    subprocess.run(["cargo", "build", "-p", "ferrowl"], cwd=REPO, check=True)

    out_dir = Path(args.out)
    for shot in shots:
        print(f"[shot] {shot.out}: {shot.desc}")
        ansi = capture_shot(shot)
        render_png(parse_ansi(ansi), out_dir / shot.out)
    print(f"{len(shots)} screenshot(s) written to {out_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
