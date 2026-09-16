#!/usr/bin/env python3
"""A mouse that is not there: wheel notches and drags through /dev/uinput.

wtype has keys but no pointer, and Hyprland can warp the pointer but not press
its buttons or turn its wheel, so what the wheel and a drag do — zooming about
the pointer, panning by hand — cannot be recorded without a device of our
own. This registers one with the kernel for as long as the command runs,
sends the events, and takes it away again. Nothing but the standard library:
the ioctls and the event record are spelled out here from linux/uinput.h and
linux/input.h.

    mouse.py wheel N          N notches: positive is away from the hand, which zooms in
    mouse.py drag DX DY       the left button held while the pointer moves DX, DY
    mouse.py click            the left button pressed and released

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


class Mouse:
    def __init__(self):
        self.fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
        fcntl.ioctl(self.fd, UI_SET_EVBIT, EV_KEY)
        fcntl.ioctl(self.fd, UI_SET_KEYBIT, BTN_LEFT)
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


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    mouse = Mouse()
    try:
        match argv[1:]:
            case ["wheel", notches]:
                mouse.wheel(int(notches))
            case ["drag", dx, dy]:
                mouse.drag(int(dx), int(dy))
            case ["click"]:
                mouse.click()
            case _:
                sys.exit(__doc__)
    finally:
        mouse.close()


if __name__ == "__main__":
    main(sys.argv)
