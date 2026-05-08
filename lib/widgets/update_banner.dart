import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';

import '../providers/app_settings_provider.dart';
import '../services/update_service.dart';

class UpdateBanner extends StatefulWidget {
  const UpdateBanner({super.key});

  @override
  State<UpdateBanner> createState() => _UpdateBannerState();
}

class _UpdateBannerState extends State<UpdateBanner> {
  bool _notesExpanded = false;
  bool _dialogShown = false;

  @override
  Widget build(BuildContext context) {
    final updateService = context.watch<UpdateService>();
    final theme = Theme.of(context);

    if (updateService.readyToInstall && !_dialogShown) {
      _dialogShown = true;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _showInstallDialog(context);
      });
    }
    if (!updateService.readyToInstall) {
      _dialogShown = false;
    }

    if (!updateService.updateAvailable) return const SizedBox.shrink();

    final hasError = updateService.errorMessage != null;
    final accentColor =
        hasError ? theme.colorScheme.error : theme.colorScheme.primary;

    return Container(
      margin: const EdgeInsets.only(bottom: 16),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: accentColor.withValues(alpha: 0.08),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: accentColor.withValues(alpha: 0.2)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(
                hasError ? Icons.error_outline : Icons.system_update_outlined,
                size: 18,
                color: accentColor,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  'Update available: v${updateService.latestVersion}',
                  style: theme.textTheme.bodyMedium?.copyWith(
                    fontWeight: FontWeight.w600,
                    color: accentColor,
                  ),
                ),
              ),
            ],
          ),

          // Error state
          if (hasError &&
              !updateService.isDownloading &&
              !updateService.isInstalling &&
              !updateService.readyToInstall) ...[
            const SizedBox(height: 8),
            Container(
              constraints: const BoxConstraints(maxHeight: 100),
              child: SingleChildScrollView(
                child: Text(
                  updateService.errorMessage!,
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: theme.colorScheme.error,
                    fontFamily:
                        'JetBrains Mono, SF Mono, Menlo, monospace',
                    fontSize: 10,
                    height: 1.4,
                  ),
                ),
              ),
            ),
            const SizedBox(height: 6),
            GestureDetector(
              onTap: () {
                Clipboard.setData(
                    ClipboardData(text: updateService.errorMessage!));
              },
              child: Text(
                'Copy diagnostic',
                style: theme.textTheme.bodySmall?.copyWith(
                  color: theme.colorScheme.error,
                  fontWeight: FontWeight.w500,
                  decoration: TextDecoration.underline,
                ),
              ),
            ),
            const SizedBox(height: 10),
            Row(
              children: [
                if (updateService.hasDownloadedFile) ...[
                  _actionButton(context,
                      label: 'Retry Install',
                      primary: true,
                      isError: true,
                      onPressed: () => updateService.proceedWithInstall()),
                  const SizedBox(width: 8),
                  _actionButton(context,
                      label: 'Install Manually',
                      isError: true,
                      onPressed: () => updateService.installManually()),
                ] else if (updateService.downloadUrl != null) ...[
                  _actionButton(context,
                      label: 'Retry',
                      primary: true,
                      isError: true,
                      onPressed: () => updateService.downloadAndInstall()),
                ],
                const SizedBox(width: 8),
                _actionButton(context,
                    label: 'View Release',
                    isError: true,
                    onPressed: () => updateService.openReleasePage()),
                const SizedBox(width: 8),
                _actionButton(context,
                    label: 'Dismiss',
                    isError: true,
                    onPressed: () => updateService.dismissUpdate()),
              ],
            ),
          ]

          // Installing state (brief — app exits soon)
          else if (updateService.isInstalling) ...[
            const SizedBox(height: 10),
            ClipRRect(
              borderRadius: BorderRadius.circular(4),
              child: LinearProgressIndicator(
                minHeight: 4,
                backgroundColor: accentColor.withValues(alpha: 0.1),
                valueColor: AlwaysStoppedAnimation<Color>(accentColor),
              ),
            ),
            const SizedBox(height: 4),
            Text(
              updateService.statusMessage ?? 'Installing...',
              style: theme.textTheme.bodySmall,
            ),
          ]

          // Downloading state
          else if (updateService.isDownloading) ...[
            const SizedBox(height: 10),
            ClipRRect(
              borderRadius: BorderRadius.circular(4),
              child: LinearProgressIndicator(
                value: updateService.downloadProgress > 0
                    ? updateService.downloadProgress
                    : null,
                minHeight: 4,
                backgroundColor: accentColor.withValues(alpha: 0.1),
                valueColor: AlwaysStoppedAnimation<Color>(accentColor),
              ),
            ),
            const SizedBox(height: 4),
            Row(
              children: [
                Expanded(
                  child: Text(
                    _downloadStatusText(updateService),
                    style: theme.textTheme.bodySmall,
                  ),
                ),
                GestureDetector(
                  onTap: () => updateService.cancelUpdate(),
                  child: Text(
                    'Cancel',
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: accentColor,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                ),
              ],
            ),
          ]

          // Ready to install state
          else if (updateService.readyToInstall) ...[
            const SizedBox(height: 8),
            Text(
              'Download complete. Ready to install.',
              style: theme.textTheme.bodySmall,
            ),
            if (updateService.downloadDiagnostic.isNotEmpty) ...[
              const SizedBox(height: 6),
              GestureDetector(
                onTap: () =>
                    setState(() => _notesExpanded = !_notesExpanded),
                child: Row(
                  children: [
                    Icon(
                      _notesExpanded
                          ? Icons.expand_less
                          : Icons.expand_more,
                      size: 16,
                      color: theme.colorScheme.outline,
                    ),
                    const SizedBox(width: 4),
                    Text(
                      'Diagnostic log',
                      style: theme.textTheme.bodySmall?.copyWith(
                        color: theme.colorScheme.outline,
                        fontWeight: FontWeight.w500,
                      ),
                    ),
                  ],
                ),
              ),
              if (_notesExpanded) ...[
                const SizedBox(height: 6),
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(10),
                  decoration: BoxDecoration(
                    color: theme.colorScheme.surface,
                    borderRadius: BorderRadius.circular(6),
                    border: Border.all(color: theme.dividerColor),
                  ),
                  constraints: const BoxConstraints(maxHeight: 150),
                  child: SingleChildScrollView(
                    child: Text(
                      updateService.downloadDiagnostic,
                      style: theme.textTheme.bodySmall?.copyWith(
                        fontFamily:
                            'JetBrains Mono, SF Mono, Menlo, monospace',
                        fontSize: 10,
                        height: 1.4,
                      ),
                    ),
                  ),
                ),
              ],
            ],
            const SizedBox(height: 10),
            Row(
              children: [
                _actionButton(context,
                    label: 'Install & Restart',
                    primary: true,
                    onPressed: () => _showInstallDialog(context)),
                const SizedBox(width: 8),
                _actionButton(context,
                    label: 'Install Manually',
                    onPressed: () => updateService.installManually()),
                const SizedBox(width: 8),
                _actionButton(context,
                    label: 'Later',
                    onPressed: () => updateService.dismissUpdate()),
              ],
            ),
          ]

          // Idle state — show release notes + actions
          else ...[
            if (updateService.releaseNotes != null &&
                updateService.releaseNotes!.trim().isNotEmpty) ...[
              const SizedBox(height: 8),
              GestureDetector(
                onTap: () => setState(() => _notesExpanded = !_notesExpanded),
                child: Row(
                  children: [
                    Icon(
                      _notesExpanded
                          ? Icons.expand_less
                          : Icons.expand_more,
                      size: 16,
                      color: theme.colorScheme.outline,
                    ),
                    const SizedBox(width: 4),
                    Text(
                      'Release notes',
                      style: theme.textTheme.bodySmall?.copyWith(
                        color: theme.colorScheme.outline,
                        fontWeight: FontWeight.w500,
                      ),
                    ),
                  ],
                ),
              ),
              if (_notesExpanded) ...[
                const SizedBox(height: 6),
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(10),
                  decoration: BoxDecoration(
                    color: theme.colorScheme.surface,
                    borderRadius: BorderRadius.circular(6),
                    border: Border.all(color: theme.dividerColor),
                  ),
                  constraints: const BoxConstraints(maxHeight: 120),
                  child: SingleChildScrollView(
                    child: Text(
                      updateService.releaseNotes!.trim(),
                      style: theme.textTheme.bodySmall?.copyWith(
                        fontFamily:
                            'JetBrains Mono, SF Mono, Menlo, monospace',
                        fontSize: 11,
                        height: 1.4,
                      ),
                    ),
                  ),
                ),
              ],
            ],
            const SizedBox(height: 10),
            Row(
              children: [
                _actionButton(
                  context,
                  label: updateService.downloadUrl != null
                      ? 'Download & Install'
                      : 'View Release',
                  primary: true,
                  onPressed: () {
                    if (updateService.downloadUrl != null) {
                      updateService.downloadAndInstall();
                    } else {
                      updateService.openReleasePage();
                    }
                  },
                ),
                const SizedBox(width: 8),
                if (updateService.downloadUrl != null)
                  _actionButton(context,
                      label: 'View Release',
                      onPressed: () => updateService.openReleasePage()),
                if (updateService.downloadUrl != null)
                  const SizedBox(width: 8),
                _actionButton(
                  context,
                  label: 'Skip',
                  onPressed: () {
                    final version = updateService.latestVersion;
                    if (version != null) {
                      context
                          .read<AppSettingsProvider>()
                          .setLastSkippedVersion(version);
                      updateService.setSkippedVersion(version);
                      updateService.dismissUpdate();
                    }
                  },
                ),
              ],
            ),
          ],
        ],
      ),
    );
  }

  void _showInstallDialog(BuildContext context) {
    final updateService = context.read<UpdateService>();
    final theme = Theme.of(context);

    showDialog(
      context: context,
      barrierDismissible: false,
      builder: (ctx) => AlertDialog(
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
        title: Row(
          children: [
            Icon(Icons.system_update_outlined,
                size: 22, color: theme.colorScheme.primary),
            const SizedBox(width: 10),
            const Text('Ready to Install'),
          ],
        ),
        content: Text(
          'Tunnel Pilot v${updateService.latestVersion} has been downloaded.\n\n'
          'The app will close and restart automatically after the update is installed.',
        ),
        actions: [
          TextButton(
            onPressed: () {
              Navigator.of(ctx).pop();
              updateService.installManually();
              _dialogShown = false;
            },
            child: const Text('Install Manually'),
          ),
          TextButton(
            onPressed: () {
              Navigator.of(ctx).pop();
              _dialogShown = false;
            },
            child: const Text('Later'),
          ),
          FilledButton(
            onPressed: () {
              Navigator.of(ctx).pop();
              updateService.proceedWithInstall();
            },
            child: const Text('Install & Restart'),
          ),
        ],
      ),
    );
  }

  String _downloadStatusText(UpdateService service) {
    if (service.statusMessage != null) return service.statusMessage!;
    final sizeText = service.downloadSizeText;
    if (service.downloadProgress > 0) {
      final pct = '${(service.downloadProgress * 100).toInt()}%';
      return sizeText.isNotEmpty ? '$pct  ·  $sizeText' : pct;
    }
    return sizeText.isNotEmpty
        ? 'Downloading...  ·  $sizeText'
        : 'Downloading...';
  }

  Widget _actionButton(
    BuildContext context, {
    required String label,
    required VoidCallback onPressed,
    bool primary = false,
    bool isError = false,
  }) {
    final theme = Theme.of(context);
    final color =
        isError ? theme.colorScheme.error : theme.colorScheme.primary;

    return GestureDetector(
      onTap: onPressed,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        decoration: BoxDecoration(
          color: primary ? color : color.withValues(alpha: 0.1),
          borderRadius: BorderRadius.circular(6),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 12,
            fontWeight: FontWeight.w500,
            color: primary ? Colors.white : color,
          ),
        ),
      ),
    );
  }
}
