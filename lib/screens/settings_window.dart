import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:window_manager/window_manager.dart';

import '../models/forward_config.dart';
// import '../providers/app_settings_provider.dart';
import '../providers/forward_provider.dart';
import '../widgets/app_settings_section.dart';
import '../widgets/backup_restore_section.dart';
import '../widgets/forward_form_dialog.dart';
import '../widgets/forward_list_tile.dart';
import '../widgets/logs_section.dart';
import '../services/update_service.dart';
import '../widgets/update_banner.dart';

class SettingsWindow extends StatefulWidget {
  const SettingsWindow({super.key});

  @override
  State<SettingsWindow> createState() => _SettingsWindowState();
}

class _SettingsWindowState extends State<SettingsWindow> {
  String? _selectedId;
  int _tabIndex = 0;

  void navigateToLogs() {
    setState(() => _tabIndex = 1);
  }

  Future<void> _addForward() async {
    final result = await showDialog<ForwardConfig>(
      context: context,
      builder: (_) => const ForwardFormDialog(),
    );
    if (result != null && mounted) {
      await context.read<ForwardProvider>().addForward(result);
    }
  }

  Future<void> _editForward(ForwardConfig config) async {
    final result = await showDialog<ForwardConfig>(
      context: context,
      builder: (_) => ForwardFormDialog(config: config),
    );
    if (result != null && mounted) {
      await context.read<ForwardProvider>().updateForward(result);
    }
  }

  Future<void> _duplicateForward() async {
    if (_selectedId == null) return;
    await context.read<ForwardProvider>().duplicateForward(_selectedId!);
  }

  Future<void> _deleteForward() async {
    if (_selectedId == null) return;

    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) {
        final theme = Theme.of(ctx);
        return Dialog(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 360),
            child: Padding(
              padding: const EdgeInsets.all(24),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text('Delete Tunnel',
                      style: theme.textTheme.titleMedium),
                  const SizedBox(height: 8),
                  Text(
                    'This tunnel configuration will be permanently deleted.',
                    style: theme.textTheme.bodyMedium?.copyWith(
                      color: theme.colorScheme.outline,
                    ),
                  ),
                  const SizedBox(height: 20),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.end,
                    children: [
                      TextButton(
                        onPressed: () => Navigator.of(ctx).pop(false),
                        child: const Text('Cancel'),
                      ),
                      const SizedBox(width: 8),
                      FilledButton(
                        onPressed: () => Navigator.of(ctx).pop(true),
                        style: FilledButton.styleFrom(
                          backgroundColor: Colors.red.shade600,
                        ),
                        child: const Text('Delete'),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
        );
      },
    );

    if (confirmed == true && mounted) {
      await context.read<ForwardProvider>().removeForward(_selectedId!);
      setState(() => _selectedId = null);
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final isDark = theme.brightness == Brightness.dark;
    final borderColor = theme.dividerColor;

    return Scaffold(
      body: Column(
        children: [
          // Nav bar
          GestureDetector(
            onPanStart: (_) => windowManager.startDragging(),
            child: Container(
              decoration: BoxDecoration(
                color: theme.colorScheme.surface,
                border: Border(bottom: BorderSide(color: borderColor)),
              ),
              padding: const EdgeInsets.fromLTRB(20, 14, 12, 0),
              child: Column(
                children: [
                  Row(
                    children: [
                      Container(
                        width: 28,
                        height: 28,
                        decoration: BoxDecoration(
                          color: theme.colorScheme.primary.withValues(alpha: isDark ? 0.15 : 0.1),
                          borderRadius: BorderRadius.circular(7),
                        ),
                        child: Icon(
                          Icons.sensors_rounded,
                          size: 16,
                          color: theme.colorScheme.primary,
                        ),
                      ),
                      const SizedBox(width: 10),
                      Text('Tunnel Pilot', style: theme.textTheme.titleMedium),
                      const Spacer(),
                      // _closeButton(theme),
                    ],
                  ),
                  const SizedBox(height: 12),
                  Row(
                    children: [
                      _navTab('Connections', 0),
                      const SizedBox(width: 4),
                      _navTab('Logs', 1),
                      const SizedBox(width: 4),
                      _navTab('Settings', 2),
                    ],
                  ),
                ],
              ),
            ),
          ),

          // Content
          Expanded(
            child: AnimatedSwitcher(
              duration: const Duration(milliseconds: 150),
              child: _tabIndex == 0
                  ? _buildConnectionsTab(context)
                  : _tabIndex == 1
                      ? _buildLogsTab(context)
                      : _buildSettingsTab(context),
            ),
          ),

          // Version footer
          Container(
            padding: const EdgeInsets.fromLTRB(20, 6, 20, 8),
            child: Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                Text(
                  'v${context.watch<UpdateService>().currentVersion}',
                  style: theme.textTheme.bodySmall?.copyWith(
                    fontSize: 11,
                    color: theme.colorScheme.outline.withValues(alpha: 0.5),
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _navTab(String label, int index) {
    final theme = Theme.of(context);
    final isActive = _tabIndex == index;

    return GestureDetector(
      onTap: () => setState(() => _tabIndex = index),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        decoration: BoxDecoration(
          border: Border(
            bottom: BorderSide(
              color: isActive ? theme.colorScheme.primary : Colors.transparent,
              width: 2,
            ),
          ),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 13,
            fontWeight: isActive ? FontWeight.w600 : FontWeight.w400,
            color: isActive
                ? theme.colorScheme.primary
                : theme.colorScheme.outline,
          ),
        ),
      ),
    );
  }

  Widget _buildConnectionsTab(BuildContext context) {
    final provider = context.watch<ForwardProvider>();
    final forwards = provider.forwards;
    final theme = Theme.of(context);

    return Column(
      key: const ValueKey('connections'),
      children: [
        // Toolbar
        Container(
          padding: const EdgeInsets.fromLTRB(20, 12, 12, 8),
          child: Row(
            children: [
              Text(
                '${forwards.length} tunnel${forwards.length == 1 ? '' : 's'}',
                style: theme.textTheme.bodySmall,
              ),
              const Spacer(),
              _toolbarAction(
                Icons.add_rounded,
                'Add',
                onPressed: _addForward,
                primary: true,
              ),
              const SizedBox(width: 6),
              _toolbarAction(
                Icons.copy_rounded,
                'Duplicate',
                onPressed: _selectedId != null ? _duplicateForward : null,
              ),
              const SizedBox(width: 6),
              _toolbarAction(
                Icons.delete_outline_rounded,
                'Delete',
                onPressed: _selectedId != null ? _deleteForward : null,
              ),
            ],
          ),
        ),

        Expanded(
          child: forwards.isEmpty
              ? _buildEmptyState(theme)
              : ListView.builder(
                  padding: const EdgeInsets.fromLTRB(20, 0, 20, 16),
                  itemCount: forwards.length,
                  itemBuilder: (context, index) {
                    final config = forwards[index];
                    final status = provider.getStatus(config.id);
                    final errorMsg = provider.getErrorMessage(config.id);

                    return ForwardListTile(
                      config: config,
                      status: status,
                      errorMessage: errorMsg,
                      isSelected: _selectedId == config.id,
                      onTap: () => setState(() => _selectedId = config.id),
                      onDoubleTap: () => _editForward(config),
                      onToggle: () => provider.toggleForward(config.id),
                      onEdit: () => _editForward(config),
                      onDuplicate: () async {
                        await provider.duplicateForward(config.id);
                      },
                      onDelete: () async {
                        setState(() => _selectedId = config.id);
                        await _deleteForward();
                      },
                      isFirst: index == 0,
                      isLast: index == forwards.length - 1,
                    );
                  },
                ),
        ),
      ],
    );
  }

  Widget _buildLogsTab(BuildContext context) {
    return const Padding(
      key: ValueKey('logs'),
      padding: EdgeInsets.fromLTRB(20, 12, 20, 16),
      child: LogsSection(),
    );
  }

  Widget _buildEmptyState(ThemeData theme) {
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(
            Icons.sensors_off_rounded,
            size: 40,
            color: theme.colorScheme.outline.withValues(alpha: 0.3),
          ),
          const SizedBox(height: 12),
          Text(
            'No tunnels yet',
            style: theme.textTheme.bodyLarge?.copyWith(
              color: theme.colorScheme.outline,
            ),
          ),
          const SizedBox(height: 4),
          Text(
            'Add your first SSH tunnel to get started',
            style: theme.textTheme.bodySmall,
          ),
          const SizedBox(height: 16),
          FilledButton.icon(
            onPressed: _addForward,
            icon: const Icon(Icons.add, size: 16),
            label: const Text('Add Tunnel'),
          ),
        ],
      ),
    );
  }

  Widget _buildSettingsTab(BuildContext context) {
    return SingleChildScrollView(
      key: const ValueKey('settings'),
      padding: const EdgeInsets.all(20),
      child: const Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          UpdateBanner(),
          AppSettingsSection(),
          SizedBox(height: 24),
          BackupRestoreSection(),
        ],
      ),
    );
  }

  Widget _toolbarAction(
    IconData icon,
    String label, {
    VoidCallback? onPressed,
    bool primary = false,
  }) {
    final theme = Theme.of(context);
    final isEnabled = onPressed != null;
    final color = isEnabled
        ? (primary ? theme.colorScheme.primary : theme.colorScheme.onSurface)
        : theme.colorScheme.outline.withValues(alpha: 0.4);

    return Tooltip(
      message: label,
      child: Material(
        color: Colors.transparent,
        borderRadius: BorderRadius.circular(6),
        child: InkWell(
          onTap: onPressed,
          borderRadius: BorderRadius.circular(6),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(icon, size: 16, color: color),
                if (primary) ...[
                  const SizedBox(width: 4),
                  Text(
                    label,
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.w500,
                      color: color,
                    ),
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}
