import 'dart:io';

/// Relative XDG values are ignored, as required by the base directory spec.
File settingsConfigFile({
  required bool isDebug,
  Directory? repositoryDirectory,
  Map<String, String>? environment,
}) {
  if (isDebug) {
    return File(
      '${(repositoryDirectory ?? Directory.current).absolute.path}/config/shared/theme.toml',
    );
  }
  final env = environment ?? Platform.environment;
  final configHome = env['XDG_CONFIG_HOME'];
  if (configHome != null && configHome.startsWith('/')) {
    return File('$configHome/remya/shared/theme.toml');
  }
  final home = env['HOME'];
  if (home == null || !home.startsWith('/')) {
    throw const FileSystemException(
      'An absolute HOME or XDG_CONFIG_HOME is required',
    );
  }
  return File('$home/.config/remya/shared/theme.toml');
}
