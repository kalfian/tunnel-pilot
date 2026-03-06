class AppSettings {
  bool launchAtLogin;
  bool showNotifications;
  String themeMode; // 'system', 'light', 'dark'

  AppSettings({
    this.launchAtLogin = false,
    this.showNotifications = true,
    this.themeMode = 'system',
  });

  Map<String, dynamic> toJson() => {
        'launchAtLogin': launchAtLogin,
        'showNotifications': showNotifications,
        'themeMode': themeMode,
      };

  factory AppSettings.fromJson(Map<String, dynamic> json) => AppSettings(
        launchAtLogin: json['launchAtLogin'] as bool? ?? false,
        showNotifications: json['showNotifications'] as bool? ?? true,
        themeMode: json['themeMode'] as String? ?? 'system',
      );
}
