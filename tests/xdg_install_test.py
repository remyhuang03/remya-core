"""Check Core installers without touching personal data or requiring real builds."""
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


def load_installer(repository):
    spec = importlib.util.spec_from_file_location(repository.name, repository / 'tool/install.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class InstallTest(unittest.TestCase):
    def test_xdg_defaults_and_relative_values(self):
        installer = load_installer(ROOT)
        self.assertEqual(installer.data_home({'HOME': '/home/test'}), Path('/home/test/.local/share'))
        self.assertEqual(installer.data_home({'HOME': '/home/test', 'XDG_DATA_HOME': 'relative'}), Path('/home/test/.local/share'))
        self.assertEqual(installer.data_home({'XDG_DATA_HOME': '/custom'}), Path('/custom'))
        with self.assertRaises(ValueError):
            installer.data_home({'HOME': 'relative'})

    def test_install_update_preserves_settings_and_rejects_unrelated_commands(self):
        installer = load_installer(ROOT)
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            home = directory / 'home'
            data = home / 'data $ %'
            root = directory / 'source'
            source = root / 'target/release'
            source.mkdir(parents=True)
            for binary in installer.BINARIES:
                (source / binary).write_text('version one ' + binary)
                (source / binary).chmod(0o755)
            (root / 'packaging').mkdir()
            (root / 'packaging/top.remya.Core1.service').write_text(
                (ROOT / 'packaging/top.remya.Core1.service').read_text())
            protected = [data / 'applications/top.remya.settings.desktop',
                         data / 'remya/remya_settings/remya_settings',
                         home / '.config/remya/shared/theme.toml',
                         home / '.local/bin/remya_settings']
            for path in protected:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('preserve me')
            environment = {'HOME': str(home), 'XDG_DATA_HOME': str(data)}
            with patch.object(installer, 'bundle_running', return_value=False), \
                 patch.object(installer.shutil, 'which', return_value=None):
                installer.install(root, environment)
                for binary in installer.BINARIES:
                    (source / binary).write_text('version two ' + binary)
                installer.install(root, environment)
            destination = data / 'remya/remya_core'
            for binary in installer.BINARIES:
                self.assertEqual((destination / binary).read_text(), 'version two ' + binary)
                self.assertTrue(os.access(destination / binary, os.X_OK))
            command = home / '.local/bin/remya'
            self.assertEqual(command.resolve(), destination / 'remya')
            for path in protected:
                self.assertEqual(path.read_text(), 'preserve me')
            unit = (data / 'systemd/user/top.remya.core.service').read_text()
            self.assertIn('Type=dbus', unit)
            self.assertIn('BusName=top.remya.Core1', unit)
            self.assertIn('WantedBy=graphical-session.target', unit)
            self.assertIn('ExecStart=' + installer.unit_quote(destination / 'remyad'), unit)
            service = (data / 'dbus-1/services/top.remya.Core1.service').read_text()
            self.assertIn('SystemdService=top.remya.core.service', service)
            self.assertNotIn('@REMYAD@', service)
            self.assertIn(str(destination / 'remyad'), service)
            with patch.object(installer, 'bundle_running', return_value=False), \
                 patch.object(installer.shutil, 'which', return_value='/usr/bin/systemctl'), \
                 patch.object(installer.subprocess, 'run') as run:
                installer.install(root, environment)
            self.assertEqual([call.args[0] for call in run.call_args_list], [
                ['systemctl', '--user', 'daemon-reload'],
                ['systemctl', '--user', 'enable', installer.UNIT],
                ['systemctl', '--user', 'restart', installer.UNIT]])
            command.unlink()
            command.write_text('unrelated program')
            with patch.object(installer, 'bundle_running', return_value=False):
                with self.assertRaisesRegex(RuntimeError, 'unrelated command'):
                    installer.install(root, environment)
            self.assertEqual(command.read_text(), 'unrelated program')
            self.assertEqual((destination / 'remyad').read_text(), 'version two remyad')

    def test_quit_refusal_aborts_before_replacement(self):
        installer = load_installer(ROOT)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'target/release').mkdir(parents=True)
            for binary in installer.BINARIES:
                (root / 'target/release' / binary).write_text('new')
            with patch.object(installer, 'bundle_running', return_value=True), \
                 patch.object(installer, 'stop_service', side_effect=RuntimeError('refused')):
                with self.assertRaisesRegex(RuntimeError, 'refused'):
                    installer.install(root, {'HOME': str(root / 'home')})
            self.assertFalse((root / 'home/.local/share/remya/remya_core').exists())


class SystemUpdaterTest(unittest.TestCase):
    def test_rule_allows_only_fixed_upgrades_for_one_valid_user(self):
        spec = importlib.util.spec_from_file_location('system_updater', ROOT / 'tool/install_system_updater.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        rule = module.rule('remy')
        self.assertIn(module.MARKER, rule)
        self.assertIn('subject.user == "remy"', rule)
        self.assertIn('command == "/usr/local/libexec/remya-core-update upgrade"', rule)
        self.assertIn('command == "/usr/local/libexec/remya-core-update upgrade --include-held"', rule)
        self.assertEqual(rule.count('polkit.Result.YES'), 1)
        for bad in ['', 'Remy', 'a" || true || "', 'x' * 40]:
            with self.assertRaises(ValueError):
                module.rule(bad)
        if os.geteuid() != 0:
            with self.assertRaisesRegex(SystemExit, 'sudo'):
                module.main(ROOT, {})


if __name__ == '__main__':
    unittest.main()
