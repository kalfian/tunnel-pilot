import 'package:flutter/material.dart';

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
    this.isFirst = false,
    this.isLast = false,
  });

  @override
  State<ForwardListTile> createState() => _ForwardListTileState();
}

class _ForwardListTileState extends State<ForwardListTile> {
  bool _hovered = false;

  Color _statusColor() {
    switch (widget.status) {
      case ForwardStatus.connected:
        return const Color(0xFF22C55E);
      case ForwardStatus.connecting:
        return const Color(0xFFF59E0B);
      case ForwardStatus.error:
        return const Color(0xFFEF4444);
      case ForwardStatus.disconnected:
        return const Color(0xFF6B7280);
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final isDark = theme.brightness == Brightness.dark;
    final hasError =
        widget.status == ForwardStatus.error && widget.errorMessage != null;
    final isActive = widget.status == ForwardStatus.connected ||
        widget.status == ForwardStatus.connecting;
    final color = _statusColor();
    final borderColor = theme.dividerColor;

    final radius = BorderRadius.vertical(
      top: widget.isFirst ? const Radius.circular(10) : Radius.zero,
      bottom: widget.isLast ? const Radius.circular(10) : Radius.zero,
    );

    return GestureDetector(
      onDoubleTap: widget.onDoubleTap,
      child: MouseRegion(
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 120),
          decoration: BoxDecoration(
            color: widget.isSelected
                ? theme.colorScheme.primary.withValues(alpha: isDark ? 0.08 : 0.05)
                : _hovered
                    ? theme.colorScheme.onSurface.withValues(alpha: 0.02)
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
          child: Material(
            color: Colors.transparent,
            child: InkWell(
              onTap: widget.onTap,
              borderRadius: radius,
              child: Padding(
                padding:
                    const EdgeInsets.symmetric(horizontal: 14, vertical: 11),
                child: Row(
                  children: [
                    // Status indicator
                    AnimatedContainer(
                      duration: const Duration(milliseconds: 200),
                      width: 7,
                      height: 7,
                      decoration: BoxDecoration(
                        color: color,
                        shape: BoxShape.circle,
                        boxShadow: isActive
                            ? [BoxShadow(color: color.withValues(alpha: 0.5), blurRadius: 6)]
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
                              style: TextStyle(
                                fontSize: 11,
                                color: Colors.red.shade400,
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
                      onTap: widget.onToggle,
                      child: AnimatedContainer(
                        duration: const Duration(milliseconds: 200),
                        width: 36,
                        height: 20,
                        decoration: BoxDecoration(
                          borderRadius: BorderRadius.circular(10),
                          color: isActive
                              ? color
                              : theme.colorScheme.outlineVariant,
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
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
