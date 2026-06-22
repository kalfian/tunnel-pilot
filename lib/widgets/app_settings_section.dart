import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app.dart';
import '../providers/app_settings_provider.dart';
import '../services/update_service.dart';

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
              icon: Icons.login_rounded,
              title: 'Launch at Login',
              subtitle: 'Start automatically when you log in',
              trailing: _customToggle(
                context: context,
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
                context: context,
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
                context: context,
                value: provider.autoReconnect,
                onChanged: (v) => provider.setAutoReconnect(v),
                activeColor: theme.colorScheme.primary,
              ),
            ),
            Divider(height: 1, color: theme.dividerColor),
            _settingsRow(
              context,
              icon: Icons.system_update_outlined,
              title: 'Auto Check for Updates',
              subtitle: 'Check for new versions periodically',
              trailing: _customToggle(
                context: context,
                value: provider.autoCheckUpdates,
                onChanged: (v) => provider.setAutoCheckUpdates(v),
                activeColor: theme.colorScheme.primary,
              ),
            ),
            Divider(height: 1, color: theme.dividerColor),
            _updateCheckRow(context),
          ],
        ),
      ],
    );
  }

  Widget _updateCheckRow(BuildContext context) {
    final theme = Theme.of(context);
    final updateService = context.watch<UpdateService>();

    String subtitle = 'Current version: v${updateService.currentVersion}';
    Color? subtitleColor;

    if (updateService.isUpToDate) {
      subtitle = 'You\'re up to date (v${updateService.currentVersion})';
      subtitleColor = context.tokens.statusConnected;
    } else if (updateService.checkError != null) {
      subtitle = updateService.checkError!;
      subtitleColor = theme.colorScheme.error;
    }

    return _settingsRow(
      context,
      icon: Icons.update_rounded,
      title: 'Check for Updates',
      subtitle: subtitle,
      subtitleColor: subtitleColor,
      trailing: updateService.isChecking
          ? SizedBox(
              width: 20,
              height: 20,
              child: CircularProgressIndicator(
                strokeWidth: 2,
                color: theme.colorScheme.primary,
              ),
            )
          : _HoverPill(
              label: 'Check',
              onTap: () {
                updateService.clearCheckStatus();
                updateService.checkForUpdate();
              },
            ),
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

    return MouseRegion(
      cursor: SystemMouseCursors.click,
      child: GestureDetector(
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
    Color? subtitleColor,
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
                  Text(
                    subtitle,
                    style: subtitleColor != null
                        ? theme.textTheme.bodySmall
                            ?.copyWith(color: subtitleColor)
                        : theme.textTheme.bodySmall,
                  ),
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
    required BuildContext context,
    required bool value,
    required ValueChanged<bool> onChanged,
    required Color activeColor,
  }) {
    final inactiveColor =
        Theme.of(context).colorScheme.outline.withValues(alpha: 0.35);
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      child: GestureDetector(
      onTap: () => onChanged(!value),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 200),
        width: 36,
        height: 20,
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(10),
          color: value ? activeColor : inactiveColor,
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
      ),
    );
  }
}

class _HoverPill extends StatefulWidget {
  final String label;
  final VoidCallback onTap;

  const _HoverPill({required this.label, required this.onTap});

  @override
  State<_HoverPill> createState() => _HoverPillState();
}

class _HoverPillState extends State<_HoverPill> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hovered = true),
      onExit: (_) => setState(() => _hovered = false),
      child: GestureDetector(
        onTap: widget.onTap,
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 120),
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
          decoration: BoxDecoration(
            color: theme.colorScheme.primary
                .withValues(alpha: _hovered ? 0.18 : 0.1),
            borderRadius: BorderRadius.circular(6),
          ),
          child: Text(
            widget.label,
            style: TextStyle(
              fontSize: 12,
              fontWeight: FontWeight.w600,
              color: theme.colorScheme.primary,
            ),
          ),
        ),
      ),
    );
  }
}
