class AppSettings {
  bool launchAtLogin;
  bool showNotifications;
  String themeMode; // 'system', 'light', 'dark'
  bool autoReconnect;
  int autoReconnectDelaySec;
  int autoReconnectMaxRetries;
  bool showInDock;
  bool autoCheckUpdates;
  String? lastSkippedVersion;

  AppSettings({
    this.launchAtLogin = true,
    this.showNotifications = true,
    this.themeMode = 'system',
    this.autoReconnect = true,
    this.autoReconnectDelaySec = 5,
    this.autoReconnectMaxRetries = 3,
    this.showInDock = false,
    this.autoCheckUpdates = true,
    this.lastSkippedVersion,
  });

  Map<String, dynamic> toJson() => {
        'launchAtLogin': launchAtLogin,
        'showNotifications': showNotifications,
        'themeMode': themeMode,
        'autoReconnect': autoReconnect,
        'autoReconnectDelaySec': autoReconnectDelaySec,
        'autoReconnectMaxRetries': autoReconnectMaxRetries,
        'showInDock': showInDock,
        'autoCheckUpdates': autoCheckUpdates,
        'lastSkippedVersion': lastSkippedVersion,
      };

  factory AppSettings.fromJson(Map<String, dynamic> json) => AppSettings(
        launchAtLogin: json['launchAtLogin'] as bool? ?? true,
        showNotifications: json['showNotifications'] as bool? ?? true,
        themeMode: json['themeMode'] as String? ?? 'system',
        autoReconnect: json['autoReconnect'] as bool? ?? true,
        autoReconnectDelaySec: json['autoReconnectDelaySec'] as int? ?? 5,
        autoReconnectMaxRetries: json['autoReconnectMaxRetries'] as int? ?? 3,
        showInDock: json['showInDock'] as bool? ?? false,
        autoCheckUpdates: json['autoCheckUpdates'] as bool? ?? true,
        lastSkippedVersion: json['lastSkippedVersion'] as String?,
      );
}
