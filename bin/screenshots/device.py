#!/usr/bin/env python3
"""A mouse and a keyboard that are not there, through /dev/uinput.

wtype has keys but no pointer, and Hyprland can warp the pointer but not press
its buttons or turn its wheel, so what the wheel and a drag do — zooming about
the pointer, panning by hand — cannot be recorded without a device of our
own. This registers one with the kernel for as long as the command runs,
sends the events, and takes it away again. Nothing but the standard library:
the ioctls and the event record are spelled out here from linux/uinput.h and
linux/input.h.

It has keys as well, for the bindings wtype cannot reach. wtype types a
keysym under a keymap of its own, at whatever keycode it chose for it, and
a binding on a key's position — Shift+2 for 50%, matched by the key being
the second digit and not by what it says — never sees the key it wants.
A key from here is the real keycode, read under the real keymap.

    device.py wheel N          N notches: positive is away from the hand, which zooms in
    device.py drag DX DY       the left button held while the pointer moves DX, DY
    device.py click            the left button pressed and released
    device.py key CHORD        a key by its position, with modifiers: 2, shift+2, ctrl+shift+c

/dev/uinput has to be writable by the user; Omarchy grants that through an
ACL, and `getfacl /dev/uinput` says whether it has.
"""

import fcntl
import os
import struct
import sys
import time

# linux/input-event-codes.h
EV_SYN, EV_KEY, EV_REL = 0x00, 0x01, 0x02
SYN_REPORT = 0
REL_X, REL_Y, REL_WHEEL, REL_WHEEL_HI_RES = 0x00, 0x01, 0x08, 0x0B
BTN_LEFT = 0x110

# The keys a chord can name, by their evdev codes.
KEYS = {
    "esc": 1, "1": 2, "2": 3, "3": 4, "4": 5, "5": 6, "6": 7, "7": 8, "8": 9,
    "9": 10, "0": 11, "minus": 12, "equal": 13, "backspace": 14, "tab": 15,
    "q": 16, "w": 17, "e": 18, "r": 19, "t": 20, "y": 21, "u": 22, "i": 23,
    "o": 24, "p": 25, "leftbrace": 26, "rightbrace": 27, "enter": 28,
    "leftctrl": 29, "a": 30, "s": 31, "d": 32, "f": 33, "g": 34, "h": 35,
    "j": 36, "k": 37, "l": 38, "semicolon": 39, "apostrophe": 40, "grave": 41,
    "leftshift": 42, "backslash": 43, "z": 44, "x": 45, "c": 46, "v": 47,
    "b": 48, "n": 49, "m": 50, "comma": 51, "dot": 52, "slash": 53,
    "rightshift": 54, "leftalt": 56, "space": 57, "up": 103, "pageup": 104,
    "left": 105, "right": 106, "end": 107, "down": 108, "pagedown": 109,
}
MODIFIERS = {"shift": "leftshift", "ctrl": "leftctrl", "alt": "leftalt"}

# linux/uinput.h, with the _IO macros already applied: 'U' is 0x55, and
# uinput_setup is an input_id (four u16), an 80-byte name and a u32.
UI_DEV_CREATE = 0x5501
UI_DEV_DESTROY = 0x5502
UI_DEV_SETUP = 0x405C5503
UI_SET_EVBIT = 0x40045564
UI_SET_KEYBIT = 0x40045565
UI_SET_RELBIT = 0x40045566

# libinput's notion of a notch, for REL_WHEEL_HI_RES.
NOTCH = 120

# How long the compositor takes to notice a device that has just appeared,
# and to drain what it sent before it goes.
SETTLE = 0.4


class Device:
    def __init__(self):
        self.fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
        fcntl.ioctl(self.fd, UI_SET_EVBIT, EV_KEY)
        fcntl.ioctl(self.fd, UI_SET_KEYBIT, BTN_LEFT)
        for code in KEYS.values():
            fcntl.ioctl(self.fd, UI_SET_KEYBIT, code)
        fcntl.ioctl(self.fd, UI_SET_EVBIT, EV_REL)
        for code in (REL_X, REL_Y, REL_WHEEL, REL_WHEEL_HI_RES):
            fcntl.ioctl(self.fd, UI_SET_RELBIT, code)
        setup = struct.pack("HHHH80sI", 0x03, 0x1234, 0x5678, 1, b"gamut screenshots", 0)
        fcntl.ioctl(self.fd, UI_DEV_SETUP, setup)
        fcntl.ioctl(self.fd, UI_DEV_CREATE)
        time.sleep(SETTLE)

    def close(self):
        time.sleep(SETTLE)
        fcntl.ioctl(self.fd, UI_DEV_DESTROY)
        os.close(self.fd)

    def emit(self, kind, code, value):
        # struct input_event: a timeval the kernel fills in, then type, code, value.
        os.write(self.fd, struct.pack("llHHi", 0, 0, kind, code, value))

    def sync(self):
        self.emit(EV_SYN, SYN_REPORT, 0)

    def wheel(self, notches):
        step = 1 if notches > 0 else -1
        for _ in range(abs(notches)):
            self.emit(EV_REL, REL_WHEEL, step)
            self.emit(EV_REL, REL_WHEEL_HI_RES, step * NOTCH)
            self.sync()
            time.sleep(0.05)

    def move(self, dx, dy):
        # In steps, so that it is a drag rather than a jump: a pointer that
        # arrives in one event is one motion event, and a drag threshold
        # or an easing that watches the pointer's path sees nothing of it.
        steps = max(1, int(max(abs(dx), abs(dy)) / 8))
        gone = [0, 0]
        for i in range(1, steps + 1):
            to = [dx * i // steps, dy * i // steps]
            self.emit(EV_REL, REL_X, to[0] - gone[0])
            self.emit(EV_REL, REL_Y, to[1] - gone[1])
            self.sync()
            gone = to
            time.sleep(0.008)

    def button(self, down):
        self.emit(EV_KEY, BTN_LEFT, 1 if down else 0)
        self.sync()

    def drag(self, dx, dy):
        self.button(True)
        time.sleep(0.05)
        self.move(dx, dy)
        time.sleep(0.05)
        self.button(False)

    def click(self):
        self.button(True)
        time.sleep(0.05)
        self.button(False)

    def key(self, chord):
        *modifiers, key = chord.lower().split("+")
        codes = [KEYS[MODIFIERS[m]] for m in modifiers] + [KEYS[key]]
        for code in codes:
            self.emit(EV_KEY, code, 1)
            self.sync()
            time.sleep(0.02)
        for code in reversed(codes):
            self.emit(EV_KEY, code, 0)
            self.sync()
            time.sleep(0.02)


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    device = Device()
    try:
        match argv[1:]:
            case ["wheel", notches]:
                device.wheel(int(notches))
            case ["drag", dx, dy]:
                device.drag(int(dx), int(dy))
            case ["click"]:
                device.click()
            case ["key", chord]:
                device.key(chord)
            case _:
                sys.exit(__doc__)
    finally:
        device.close()


if __name__ == "__main__":
    main(sys.argv)
