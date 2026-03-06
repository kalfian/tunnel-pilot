import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../providers/forward_provider.dart';

class BackupRestoreSection extends StatelessWidget {
  const BackupRestoreSection({super.key});

  Future<void> _exportBackup(BuildContext context) async {
    final provider = context.read<ForwardProvider>();

    final path = await FilePicker.platform.saveFile(
      dialogTitle: 'Export Backup',
      fileName: 'tunnel_pilot_backup.json',
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
              _actionRow(
                context,
                icon: Icons.upload_outlined,
                title: 'Export Backup',
                subtitle: 'Save configurations as JSON (passwords excluded)',
                onTap: () => _exportBackup(context),
              ),
              Divider(height: 1, color: theme.dividerColor),
              _actionRow(
                context,
                icon: Icons.download_outlined,
                title: 'Import Backup',
                subtitle: 'Restore configurations from a JSON file',
                onTap: () => _importBackup(context),
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _actionRow(
    BuildContext context, {
    required IconData icon,
    required String title,
    required String subtitle,
    required VoidCallback onTap,
  }) {
    final theme = Theme.of(context);

    return Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(10),
        child: Padding(
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
                    const SizedBox(height: 1),
                    Text(subtitle, style: theme.textTheme.bodySmall),
                  ],
                ),
              ),
              Icon(Icons.chevron_right_rounded,
                  size: 18, color: theme.colorScheme.outline),
            ],
          ),
        ),
      ),
    );
  }
}
