import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../models/forward_config.dart';
import '../models/forward_status.dart';

class ForwardListTile extends StatefulWidget {
  final ForwardConfig config;
  final ForwardStatus status;
  final String? errorMessage;
  final VoidCallback onToggle;
  final VoidCallback onDoubleTap;
  final bool isSelected;
  final VoidCallback onTap;
  final VoidCallback onEdit;
  final VoidCallback onDuplicate;
  final VoidCallback onDelete;
  final bool isFirst;
  final bool isLast;

  const ForwardListTile({
    super.key,
    required this.config,
    required this.status,
    this.errorMessage,
    required this.onToggle,
    required this.onDoubleTap,
    required this.isSelected,
    required this.onTap,
    required this.onEdit,
    required this.onDuplicate,
    required this.onDelete,
    this.isFirst = false,
    this.isLast = false,
  });

  @override
  State<ForwardListTile> createState() => _ForwardListTileState();
}

class _ForwardListTileState extends State<ForwardListTile> {
  Color _statusColor() {
    switch (widget.status) {
      case ForwardStatus.connected:
        return const Color(0xFF22C55E);
      case ForwardStatus.connecting:
      case ForwardStatus.disconnecting:
        return const Color(0xFFF59E0B);
      case ForwardStatus.error:
        return const Color(0xFFEF4444);
      case ForwardStatus.disconnected:
        return const Color(0xFF6B7280);
    }
  }

  void _showContextMenu(BuildContext context, Offset position) async {
    final theme = Theme.of(context);
    final result = await showMenu<String>(
      context: context,
      position: RelativeRect.fromLTRB(
        position.dx,
        position.dy,
        position.dx,
        position.dy,
      ),
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
      items: [
        PopupMenuItem(
          value: 'edit',
          height: 36,
          child: Row(
            children: [
              Icon(Icons.edit_outlined, size: 16, color: theme.colorScheme.onSurface),
              const SizedBox(width: 8),
              const Text('Edit', style: TextStyle(fontSize: 13)),
            ],
          ),
        ),
        PopupMenuItem(
          value: 'duplicate',
          height: 36,
          child: Row(
            children: [
              Icon(Icons.copy_rounded, size: 16, color: theme.colorScheme.onSurface),
              const SizedBox(width: 8),
              const Text('Duplicate', style: TextStyle(fontSize: 13)),
            ],
          ),
        ),
        PopupMenuItem(
          value: 'copy_ssh',
          height: 36,
          child: Row(
            children: [
              Icon(Icons.terminal_rounded,
                  size: 16, color: theme.colorScheme.onSurface),
              const SizedBox(width: 8),
              const Text('Copy SSH Command', style: TextStyle(fontSize: 13)),
            ],
          ),
        ),
        const PopupMenuDivider(height: 8),
        PopupMenuItem(
          value: 'delete',
          height: 36,
          child: Row(
            children: const [
              Icon(Icons.delete_outline_rounded,
                  size: 16, color: Color(0xFFEF4444)),
              SizedBox(width: 8),
              Text('Delete',
                  style: TextStyle(fontSize: 13, color: Color(0xFFEF4444))),
            ],
          ),
        ),
      ],
    );

    if (!context.mounted) return;

    switch (result) {
      case 'edit':
        widget.onEdit();
        break;
      case 'duplicate':
        widget.onDuplicate();
        break;
      case 'copy_ssh':
        await _copySshCommand(context);
        break;
      case 'delete':
        widget.onDelete();
        break;
    }
  }

  Future<void> _copySshCommand(BuildContext context) async {
    final command = widget.config.toSshCommand();
    await Clipboard.setData(ClipboardData(text: command));
    if (!context.mounted) return;
    final messenger = ScaffoldMessenger.maybeOf(context);
    messenger?.hideCurrentSnackBar();
    messenger?.showSnackBar(
      SnackBar(
        content: const Text('SSH command copied to clipboard'),
        duration: const Duration(seconds: 2),
        behavior: SnackBarBehavior.floating,
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final isDark = theme.brightness == Brightness.dark;
    final hasError =
        widget.status == ForwardStatus.error && widget.errorMessage != null;
    final isTransitioning = widget.status == ForwardStatus.connecting ||
        widget.status == ForwardStatus.disconnecting;
    final isActive = widget.status == ForwardStatus.connected || isTransitioning;
    final color = _statusColor();
    final borderColor = theme.dividerColor;

    final radius = BorderRadius.vertical(
      top: widget.isFirst ? const Radius.circular(10) : Radius.zero,
      bottom: widget.isLast ? const Radius.circular(10) : Radius.zero,
    );

    return GestureDetector(
      onTap: widget.onTap,
      onDoubleTap: widget.onDoubleTap,
      onSecondaryTapUp: (details) =>
          _showContextMenu(context, details.globalPosition),
      child: Container(
        decoration: BoxDecoration(
          color: widget.isSelected
              ? theme.colorScheme.primary
                  .withValues(alpha: isDark ? 0.08 : 0.05)
              : theme.colorScheme.surface,
          borderRadius: radius,
          border: Border(
            left: BorderSide(color: borderColor),
            right: BorderSide(color: borderColor),
            top: widget.isFirst
                ? BorderSide(color: borderColor)
                : BorderSide(color: borderColor, width: 0.5),
            bottom: widget.isLast
                ? BorderSide(color: borderColor)
                : BorderSide.none,
          ),
        ),
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 11),
        child: Row(
          children: [
            // Status indicator
            if (isTransitioning)
              SizedBox(
                width: 7,
                height: 7,
                child: CircularProgressIndicator(
                  strokeWidth: 1.5,
                  color: color,
                ),
              )
            else
              AnimatedContainer(
                duration: const Duration(milliseconds: 200),
                width: 7,
                height: 7,
                decoration: BoxDecoration(
                  color: color,
                  shape: BoxShape.circle,
                  boxShadow: isActive
                      ? [
                          BoxShadow(
                              color: color.withValues(alpha: 0.5),
                              blurRadius: 6)
                        ]
                      : null,
                ),
              ),
            const SizedBox(width: 12),

            // Info
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  Row(
                    children: [
                      Flexible(
                        child: Text(
                          widget.config.name,
                          style: theme.textTheme.titleSmall,
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                      if (widget.config.needsPassword) ...[
                        const SizedBox(width: 6),
                        const Icon(Icons.key_off_rounded,
                            size: 13, color: Color(0xFFF59E0B)),
                      ],
                    ],
                  ),
                  const SizedBox(height: 1),
                  Text(
                    ':${widget.config.localPort} \u2192 ${widget.config.remoteHost}:${widget.config.remotePort}',
                    style: TextStyle(
                      fontSize: 11,
                      fontFamily: 'JetBrains Mono, SF Mono, Menlo, monospace',
                      color: theme.colorScheme.outline,
                    ),
                    overflow: TextOverflow.ellipsis,
                  ),
                  if (hasError) ...[
                    const SizedBox(height: 2),
                    Text(
                      widget.errorMessage!,
                      style: const TextStyle(
                        fontSize: 11,
                        color: Color(0xFFEF4444),
                      ),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ],
                ],
              ),
            ),

            const SizedBox(width: 12),

            // Custom toggle
            GestureDetector(
              onTap: isTransitioning ? null : widget.onToggle,
              child: AnimatedOpacity(
                duration: const Duration(milliseconds: 200),
                opacity: isTransitioning ? 0.5 : 1.0,
                child: AnimatedContainer(
                duration: const Duration(milliseconds: 200),
                width: 36,
                height: 20,
                decoration: BoxDecoration(
                  borderRadius: BorderRadius.circular(10),
                  color: isActive ? color : theme.colorScheme.outlineVariant,
                ),
                child: AnimatedAlign(
                  duration: const Duration(milliseconds: 200),
                  curve: Curves.easeInOut,
                  alignment: isActive
                      ? Alignment.centerRight
                      : Alignment.centerLeft,
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
            ),
          ],
        ),
      ),
    );
  }
}
