import 'dart:io';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../providers/app_settings_provider.dart';

class AppSettingsSection extends StatelessWidget {
  const AppSettingsSection({super.key});

  @override
  Widget build(BuildContext context) {
    final provider = context.watch<AppSettingsProvider>();
    final theme = Theme.of(context);
    final isDark = theme.brightness == Brightness.dark;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('General', style: theme.textTheme.labelSmall),
        const SizedBox(height: 10),
        _settingsCard(
          context,
          children: [
            _settingsRow(
              context,
              icon: Icons.palette_outlined,
              title: 'Appearance',
              trailing: _themePicker(context, provider, isDark),
            ),
            Divider(height: 1, color: theme.dividerColor),
            _settingsRow(
              context,
              icon: Icons.rocket_launch_outlined,
              title: 'Launch at Login',
              subtitle: 'Start automatically when you log in',
              trailing: _customToggle(
                value: provider.launchAtLogin,
                onChanged: (v) => provider.setLaunchAtLogin(v),
                activeColor: theme.colorScheme.primary,
              ),
            ),
            Divider(height: 1, color: theme.dividerColor),
            _settingsRow(
              context,
              icon: Icons.notifications_outlined,
              title: 'Notifications',
              subtitle: 'Show connection status changes',
              trailing: _customToggle(
                value: provider.showNotifications,
                onChanged: (v) => provider.setShowNotifications(v),
                activeColor: theme.colorScheme.primary,
              ),
            ),
            Divider(height: 1, color: theme.dividerColor),
            _settingsRow(
              context,
              icon: Icons.refresh_rounded,
              title: 'Auto Reconnect',
              subtitle: 'Retry ${provider.autoReconnectMaxRetries}x after ${provider.autoReconnectDelaySec}s delay',
              trailing: _customToggle(
                value: provider.autoReconnect,
                onChanged: (v) => provider.setAutoReconnect(v),
                activeColor: theme.colorScheme.primary,
              ),
            ),
            Divider(height: 1, color: theme.dividerColor),
            _settingsRow(
              context,
              icon: Platform.isMacOS
                  ? Icons.dock_outlined
                  : Icons.desktop_windows_outlined,
              title: Platform.isMacOS
                  ? 'Show in Dock'
                  : 'Show in Taskbar',
              subtitle: Platform.isMacOS
                  ? 'Show app icon in Dock when window is open'
                  : 'Show app icon in taskbar when window is open',
              trailing: _customToggle(
                value: provider.showInDock,
                onChanged: (v) => provider.setShowInDock(v),
                activeColor: theme.colorScheme.primary,
              ),
            ),
          ],
        ),
      ],
    );
  }

  Widget _themePicker(
      BuildContext context, AppSettingsProvider provider, bool isDark) {
    final current = provider.themeMode;

    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        _themeOption(context, 'system', Icons.brightness_auto_outlined, current,
            (v) => provider.setThemeMode(v)),
        const SizedBox(width: 4),
        _themeOption(context, 'light', Icons.light_mode_outlined, current,
            (v) => provider.setThemeMode(v)),
        const SizedBox(width: 4),
        _themeOption(context, 'dark', Icons.dark_mode_outlined, current,
            (v) => provider.setThemeMode(v)),
      ],
    );
  }

  Widget _themeOption(BuildContext context, String mode, IconData icon,
      String current, ValueChanged<String> onChanged) {
    final theme = Theme.of(context);
    final isSelected = current == mode;

    return GestureDetector(
      onTap: () => onChanged(mode),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 150),
        width: 32,
        height: 28,
        decoration: BoxDecoration(
          color: isSelected
              ? theme.colorScheme.primary.withValues(alpha: 0.12)
              : Colors.transparent,
          borderRadius: BorderRadius.circular(6),
          border: Border.all(
            color: isSelected
                ? theme.colorScheme.primary.withValues(alpha: 0.4)
                : theme.dividerColor,
          ),
        ),
        child: Icon(
          icon,
          size: 15,
          color: isSelected
              ? theme.colorScheme.primary
              : theme.colorScheme.outline,
        ),
      ),
    );
  }

  Widget _settingsCard(BuildContext context, {required List<Widget> children}) {
    final theme = Theme.of(context);

    return Container(
      decoration: BoxDecoration(
        color: theme.colorScheme.surface,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: theme.dividerColor),
      ),
      child: Column(children: children),
    );
  }

  Widget _settingsRow(
    BuildContext context, {
    required IconData icon,
    required String title,
    String? subtitle,
    required Widget trailing,
  }) {
    final theme = Theme.of(context);

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
      child: Row(
        children: [
          Icon(icon, size: 18, color: theme.colorScheme.outline),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(title, style: theme.textTheme.bodyMedium),
                if (subtitle != null) ...[
                  const SizedBox(height: 1),
                  Text(subtitle, style: theme.textTheme.bodySmall),
                ],
              ],
            ),
          ),
          const SizedBox(width: 12),
          trailing,
        ],
      ),
    );
  }

  Widget _customToggle({
    required bool value,
    required ValueChanged<bool> onChanged,
    required Color activeColor,
  }) {
    return GestureDetector(
      onTap: () => onChanged(!value),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 200),
        width: 36,
        height: 20,
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(10),
          color: value ? activeColor : const Color(0xFFD1D5DB),
        ),
        child: AnimatedAlign(
          duration: const Duration(milliseconds: 200),
          curve: Curves.easeInOut,
          alignment: value ? Alignment.centerRight : Alignment.centerLeft,
          child: Container(
            width: 16,
            height: 16,
            margin: const EdgeInsets.symmetric(horizontal: 2),
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              color: Colors.white,
              boxShadow: [
                BoxShadow(
                  color: Colors.black.withValues(alpha: 0.15),
                  blurRadius: 2,
                  offset: const Offset(0, 1),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
