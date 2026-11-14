import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';

import 'package:material_ui/material_ui.dart';

import 'surfaces.dart';

/// The centered application identity at the top of an About page.
class RemyaAboutHeader extends StatelessWidget {
  const RemyaAboutHeader({
    super.key,
    required this.icon,
    required this.name,
    this.version,
  });

  /// The application icon, drawn 104 px square, such as
  /// `Image.asset('assets/icons/app_icon.png', semanticLabel: ...)`.
  final Widget icon;
  final String name;

  /// Shown as `v<version>`; omitted while unknown.
  final String? version;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    return Padding(
      padding: const .fromLTRB(20, 36, 20, 36),
      child: Column(
        children: [
          SizedBox.square(dimension: 104, child: icon),
          const SizedBox(height: 20),
          Text(name, style: text.headlineMedium, textAlign: .center),
          const SizedBox(height: 8),
          if (version case final version?)
            Text(
              'v$version',
              style: text.labelLarge?.copyWith(
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
            ),
        ],
      ),
    );
  }
}

/// Repository, Third-party licenses and Project license rows of an About page.
class RemyaAboutLinks extends StatelessWidget {
  const RemyaAboutLinks({
    super.key,
    required this.repository,
    required this.onLicenses,
    this.license = 'PolyForm Noncommercial License 1.0.0',
  });

  /// Fixed repository URL opened with the desktop opener.
  final String repository;
  final VoidCallback onLicenses;
  final String license;

  Future<void> _open(BuildContext context) async {
    try {
      final result = await Process.run('xdg-open', [repository]);
      if (result.exitCode == 0) return;
    } on ProcessException {
      // The desktop opener may be missing or unavailable.
    }
    if (context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Unable to open the repository.')),
      );
    }
  }

  @override
  Widget build(BuildContext context) => SettingsSurface(
    padding: .zero,
    child: settingsRows([
      SettingsLinkRow(
        key: const ValueKey('about-repository'),
        icon: Icons.code_rounded,
        title: 'Repository',
        subtitle: repository,
        external: true,
        onTap: () => _open(context),
      ),
      SettingsLinkRow(
        key: const ValueKey('about-licenses'),
        icon: Icons.description_outlined,
        title: 'Third-party licenses',
        subtitle: 'View licenses for Flutter and dependencies',
        onTap: onLicenses,
      ),
      SettingsLinkRow(
        icon: Icons.policy_outlined,
        title: 'Project license',
        subtitle: license,
      ),
    ]),
  );
}

/// The running application's version from the `version.json` Flutter bundles
/// next to its assets, or null when it is missing (for example in tests).
Future<String?> remyaAppVersion() async {
  try {
    final json = await rootBundle.loadString('version.json');
    final version = (jsonDecode(json) as Map<String, dynamic>)['version'];
    return version is String && version.isNotEmpty ? version : null;
  } on Object {
    return null;
  }
}
