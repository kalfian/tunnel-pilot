import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';

import '../services/log_service.dart';

class LogsSection extends StatelessWidget {
  const LogsSection({super.key});

  @override
  Widget build(BuildContext context) {
    final logService = context.watch<LogService>();
    final logs = logService.logs;
    final theme = Theme.of(context);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Header row with actions
        Row(
          children: [
            Text(
              '${logs.length} log entries',
              style: theme.textTheme.bodySmall,
            ),
            const Spacer(),
            if (logs.isNotEmpty) ...[
              _actionButton(
                context,
                icon: Icons.copy_rounded,
                label: 'Copy All',
                onTap: () {
                  Clipboard.setData(ClipboardData(text: logService.allLogsText));
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(
                      content: const Text('Logs copied to clipboard'),
                      duration: const Duration(seconds: 2),
                      behavior: SnackBarBehavior.floating,
                      margin: const EdgeInsets.all(16),
                    ),
                  );
                },
              ),
              const SizedBox(width: 6),
              _actionButton(
                context,
                icon: Icons.delete_outline_rounded,
                label: 'Clear',
                onTap: () => logService.clear(),
              ),
            ],
          ],
        ),
        const SizedBox(height: 12),

        // Log list
        Expanded(
          child: logs.isEmpty
              ? Center(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(
                        Icons.article_outlined,
                        size: 40,
                        color: theme.colorScheme.outline.withValues(alpha: 0.3),
                      ),
                      const SizedBox(height: 12),
                      Text(
                        'No logs yet',
                        style: theme.textTheme.bodyLarge?.copyWith(
                          color: theme.colorScheme.outline,
                        ),
                      ),
                      const SizedBox(height: 4),
                      Text(
                        'Connect a tunnel to see activity logs',
                        style: theme.textTheme.bodySmall,
                      ),
                    ],
                  ),
                )
              : Container(
                  decoration: BoxDecoration(
                    color: theme.colorScheme.surface,
                    borderRadius: BorderRadius.circular(10),
                    border: Border.all(color: theme.dividerColor),
                  ),
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(10),
                    child: ListView.builder(
                      padding: EdgeInsets.zero,
                      itemCount: logs.length,
                      itemBuilder: (context, index) {
                        final log = logs[index];
                        return _LogRow(
                          log: log,
                          isFirst: index == 0,
                          showDivider: index < logs.length - 1,
                        );
                      },
                    ),
                  ),
                ),
        ),
      ],
    );
  }

  Widget _actionButton(
    BuildContext context, {
    required IconData icon,
    required String label,
    required VoidCallback onTap,
  }) {
    final theme = Theme.of(context);

    return Tooltip(
      message: label,
      child: Material(
        color: Colors.transparent,
        borderRadius: BorderRadius.circular(6),
        child: InkWell(
          onTap: onTap,
          borderRadius: BorderRadius.circular(6),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(icon, size: 14, color: theme.colorScheme.outline),
                const SizedBox(width: 4),
                Text(
                  label,
                  style: TextStyle(
                    fontSize: 11,
                    fontWeight: FontWeight.w500,
                    color: theme.colorScheme.outline,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _LogRow extends StatelessWidget {
  final LogEntry log;
  final bool isFirst;
  final bool showDivider;

  const _LogRow({
    required this.log,
    required this.isFirst,
    required this.showDivider,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    Color levelColor;
    IconData levelIcon;
    switch (log.level) {
      case LogLevel.info:
        levelColor = const Color(0xFF22C55E);
        levelIcon = Icons.check_circle_outline_rounded;
      case LogLevel.warning:
        levelColor = const Color(0xFFF59E0B);
        levelIcon = Icons.warning_amber_rounded;
      case LogLevel.error:
        levelColor = const Color(0xFFEF4444);
        levelIcon = Icons.error_outline_rounded;
    }

    return Column(
      children: [
        InkWell(
          onTap: () {
            Clipboard.setData(ClipboardData(text: log.formattedLine));
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(
                content: const Text('Log entry copied'),
                duration: const Duration(seconds: 1),
                behavior: SnackBarBehavior.floating,
                margin: const EdgeInsets.all(16),
              ),
            );
          },
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 8),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Icon(levelIcon, size: 14, color: levelColor),
                const SizedBox(width: 8),
                Text(
                  log.formattedTime,
                  style: TextStyle(
                    fontSize: 11,
                    fontFamily: 'SF Mono',
                    color: theme.colorScheme.outline,
                  ),
                ),
                const SizedBox(width: 10),
                Container(
                  padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
                  decoration: BoxDecoration(
                    color: theme.colorScheme.primary.withValues(alpha: 0.1),
                    borderRadius: BorderRadius.circular(4),
                  ),
                  child: Text(
                    log.tunnelName,
                    style: TextStyle(
                      fontSize: 10,
                      fontWeight: FontWeight.w600,
                      color: theme.colorScheme.primary,
                    ),
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Text(
                    log.message,
                    style: TextStyle(
                      fontSize: 12,
                      color: theme.colorScheme.onSurface,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
        if (showDivider) Divider(height: 1, color: theme.dividerColor),
      ],
    );
  }
}
