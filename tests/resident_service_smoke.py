"""Verify standalone Core residency and reload PSS on an isolated bus."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BINARIES = ROOT / 'target/release'


def pss(pid):
    fields = dict(line.split(':', 1) for line in Path(f'/proc/{pid}/smaps_rollup').read_text().splitlines() if ':' in line)
    return int(fields['Pss'].split()[0])


def checks():
    service = subprocess.Popen([str(BINARIES / 'remyad')])
    cli = BINARIES / 'remya'
    def call(*args):
        result = subprocess.run([str(cli), *args], capture_output=True, text=True, timeout=35)
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)
    try:
        deadline = time.monotonic() + 10
        while True:
            try:
                call('get', 'theme')
                break
            except AssertionError:
                if time.monotonic() > deadline:
                    raise
                time.sleep(.1)
        initial = pss(service.pid)
        for _ in range(3):
            call('reload')
            assert call('get', 'date-time')['timezone']
        closed = pss(service.pid)
        assert closed - initial < 5 * 1024, 'Resident memory grew unexpectedly'
        output = {'initial_pss_kib': initial, 'after_reload_pss_kib': closed}
        print('PASS: standalone Core residency and reload:', output)
        call('quit')
        service.wait(timeout=10)
    finally:
        if service.poll() is None:
            service.terminate()
            service.wait(timeout=10)


if __name__ == '__main__':
    if '--session' in sys.argv:
        checks()
    else:
        with tempfile.TemporaryDirectory(prefix='remya-resident-') as temporary:
            environment = dict(os.environ, HOME=temporary, XDG_CONFIG_HOME=temporary + '/config',
                               XDG_DATA_HOME=temporary + '/data', GSETTINGS_BACKEND='keyfile',
                               GIO_USE_VFS='local', NO_AT_BRIDGE='1', GTK_USE_PORTAL='0')
            subprocess.run(['dbus-run-session', '--', sys.executable, __file__, '--session'], env=environment, check=True, timeout=90)
