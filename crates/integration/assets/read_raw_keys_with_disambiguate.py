#!/usr/bin/env python3
"""Key-reading script with only Kitty keyboard protocol flag 1 (disambiguate escape codes).

Enables the protocol, then prints the exact bytes of every key press as `KEY: <repr>`. Used by
test_keyboard_protocol_modified_editing_keys_reach_app to verify that Cmd/Option-modified
Backspace, Delete and arrow keys reach the app with their modifiers instead of being rewritten
into shell line-editing shortcuts.
"""
import os
import sys
import termios
import tty

fd = sys.stdin.fileno()
old_settings = termios.tcgetattr(fd)
try:
    tty.setraw(fd)
    sys.stdout.write('\x1b[=1u')
    sys.stdout.write('Protocol enabled\r\n')
    sys.stdout.flush()

    while True:
        data = os.read(fd, 64)
        sys.stdout.write(f'KEY: {data!r}\r\n')
        sys.stdout.flush()
        if data == b'\x03':
            break
finally:
    termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)
    sys.stdout.write('\x1b[=0u')
