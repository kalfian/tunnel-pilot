import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app.dart';
import '../providers/forward_provider.dart';

class BackupRestoreSection extends StatelessWidget {
  const BackupRestoreSection({super.key});

  /// Default the backup dialogs to the user's home folder so they don't
  /// inherit the file picker's last-used location (e.g. ~/.ssh from the
  /// identity-file picker). Returns null if home can't be resolved, which
  /// lets the OS pick its own default.
  String? _defaultDirectory() {
    final home =
        Platform.environment['HOME'] ?? Platform.environment['USERPROFILE'];
    return (home != null && home.isNotEmpty) ? home : null;
  }

  Future<void> _exportBackup(BuildContext context) async {
    final provider = context.read<ForwardProvider>();

    final path = await FilePicker.platform.saveFile(
      dialogTitle: 'Export Backup',
      fileName: 'tunnel_pilot_backup.json',
      initialDirectory: _defaultDirectory(),
      type: FileType.custom,
      allowedExtensions: ['json'],
    );

    if (path == null) return;

    try {
      await provider.exportBackup(path);
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: const Text('Backup exported successfully'),
            behavior: SnackBarBehavior.floating,
            shape:
                RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
          ),
        );
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Export failed: $e'),
            behavior: SnackBarBehavior.floating,
          ),
        );
      }
    }
  }

  Future<void> _importBackup(BuildContext context) async {
    final provider = context.read<ForwardProvider>();

    final result = await FilePicker.platform.pickFiles(
      dialogTitle: 'Import Backup',
      initialDirectory: _defaultDirectory(),
      type: FileType.custom,
      allowedExtensions: ['json'],
    );

    if (result == null || result.files.single.path == null) return;

    try {
      final imported = await provider.importBackup(result.files.single.path!);
      final needPassword = imported.where((f) => f.needsPassword).length;

      if (context.mounted) {
        var message = 'Imported ${imported.length} tunnel(s).';
        if (needPassword > 0) {
          message += ' $needPassword need password re-entry.';
        }
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(message),
            duration: const Duration(seconds: 4),
            behavior: SnackBarBehavior.floating,
            shape:
                RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
          ),
        );
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Import failed: $e'),
            behavior: SnackBarBehavior.floating,
          ),
        );
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Data', style: theme.textTheme.labelSmall),
        const SizedBox(height: 10),
        Container(
          decoration: BoxDecoration(
            color: theme.colorScheme.surface,
            borderRadius: BorderRadius.circular(10),
            border: Border.all(color: theme.dividerColor),
          ),
          child: Column(
            children: [
              _ActionRow(
                icon: Icons.upload_outlined,
                title: 'Export Backup',
                subtitle: 'Save configurations as JSON (passwords excluded)',
                onTap: () => _exportBackup(context),
                topRadius: true,
              ),
              Divider(height: 1, color: theme.dividerColor),
              _ActionRow(
                icon: Icons.download_outlined,
                title: 'Import Backup',
                subtitle: 'Restore configurations from a JSON file',
                onTap: () => _importBackup(context),
                bottomRadius: true,
              ),
            ],
          ),
        ),
      ],
    );
  }

}

class _ActionRow extends StatefulWidget {
  final IconData icon;
  final String title;
  final String subtitle;
  final VoidCallback onTap;
  final bool topRadius;
  final bool bottomRadius;

  const _ActionRow({
    required this.icon,
    required this.title,
    required this.subtitle,
    required this.onTap,
    this.topRadius = false,
    this.bottomRadius = false,
  });

  @override
  State<_ActionRow> createState() => _ActionRowState();
}

class _ActionRowState extends State<_ActionRow> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final tokens = context.tokens;

    final radius = BorderRadius.vertical(
      top: widget.topRadius ? const Radius.circular(10) : Radius.zero,
      bottom: widget.bottomRadius ? const Radius.circular(10) : Radius.zero,
    );

    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hovered = true),
      onExit: (_) => setState(() => _hovered = false),
      child: GestureDetector(
        // Whole padded row should be tappable, not just the text/icon glyphs.
        // Without this, the transparent background + deferToChild hit-testing
        // leaves the padding and the title→chevron gap as dead click zones.
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 120),
          decoration: BoxDecoration(
            color: _hovered ? tokens.hover : Colors.transparent,
            borderRadius: radius,
          ),
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
          child: Row(
            children: [
              Icon(widget.icon, size: 18, color: theme.colorScheme.outline),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(widget.title, style: theme.textTheme.bodyMedium),
                    const SizedBox(height: 1),
                    Text(widget.subtitle, style: theme.textTheme.bodySmall),
                  ],
                ),
              ),
              AnimatedSlide(
                duration: const Duration(milliseconds: 120),
                offset: _hovered ? const Offset(0.15, 0) : Offset.zero,
                child: Icon(
                  Icons.chevron_right_rounded,
                  size: 18,
                  color: theme.colorScheme.outline
                      .withValues(alpha: _hovered ? 1 : 0.5),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
