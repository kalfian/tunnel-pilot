import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../providers/app_settings_provider.dart';
import '../services/update_service.dart';

class UpdateBanner extends StatelessWidget {
  const UpdateBanner({super.key});

  @override
  Widget build(BuildContext context) {
    final updateService = context.watch<UpdateService>();
    final theme = Theme.of(context);

    if (!updateService.updateAvailable) return const SizedBox.shrink();

    return Container(
      margin: const EdgeInsets.only(bottom: 16),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: updateService.errorMessage != null
            ? theme.colorScheme.error.withValues(alpha: 0.08)
            : theme.colorScheme.primary.withValues(alpha: 0.08),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(
          color: updateService.errorMessage != null
              ? theme.colorScheme.error.withValues(alpha: 0.2)
              : theme.colorScheme.primary.withValues(alpha: 0.2),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(
                updateService.errorMessage != null
                    ? Icons.error_outline
                    : Icons.system_update_outlined,
                size: 18,
                color: updateService.errorMessage != null
                    ? theme.colorScheme.error
                    : theme.colorScheme.primary,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  'Update available: v${updateService.latestVersion}',
                  style: theme.textTheme.bodyMedium?.copyWith(
                    fontWeight: FontWeight.w600,
                    color: updateService.errorMessage != null
                        ? theme.colorScheme.error
                        : theme.colorScheme.primary,
                  ),
                ),
              ),
            ],
          ),
          if (updateService.errorMessage != null &&
              !updateService.isDownloading &&
              !updateService.isInstalling) ...[
            const SizedBox(height: 8),
            Text(
              updateService.errorMessage!,
              style: theme.textTheme.bodySmall?.copyWith(
                color: theme.colorScheme.error,
              ),
            ),
            const SizedBox(height: 10),
            Row(
              children: [
                if (updateService.downloadUrl != null)
                  _actionButton(
                    context,
                    label: 'Retry',
                    primary: true,
                    isError: true,
                    onPressed: () => updateService.downloadAndInstall(),
                  ),
                if (updateService.downloadUrl != null)
                  const SizedBox(width: 8),
                _actionButton(
                  context,
                  label: 'View Release',
                  isError: true,
                  onPressed: () => updateService.openReleasePage(),
                ),
                const SizedBox(width: 8),
                _actionButton(
                  context,
                  label: 'Dismiss',
                  isError: true,
                  onPressed: () => updateService.dismissUpdate(),
                ),
              ],
            ),
          ] else if (updateService.isInstalling) ...[
            const SizedBox(height: 10),
            ClipRRect(
              borderRadius: BorderRadius.circular(4),
              child: LinearProgressIndicator(
                minHeight: 4,
                backgroundColor:
                    theme.colorScheme.primary.withValues(alpha: 0.1),
                valueColor: AlwaysStoppedAnimation<Color>(
                  theme.colorScheme.primary,
                ),
              ),
            ),
            const SizedBox(height: 4),
            Text(
              updateService.statusMessage ?? 'Installing...',
              style: theme.textTheme.bodySmall,
            ),
          ] else if (updateService.isDownloading) ...[
            const SizedBox(height: 10),
            ClipRRect(
              borderRadius: BorderRadius.circular(4),
              child: LinearProgressIndicator(
                value: updateService.downloadProgress > 0
                    ? updateService.downloadProgress
                    : null,
                minHeight: 4,
                backgroundColor:
                    theme.colorScheme.primary.withValues(alpha: 0.1),
                valueColor: AlwaysStoppedAnimation<Color>(
                  theme.colorScheme.primary,
                ),
              ),
            ),
            const SizedBox(height: 4),
            Row(
              children: [
                Expanded(
                  child: Text(
                    updateService.downloadProgress > 0
                        ? '${(updateService.downloadProgress * 100).toInt()}%'
                        : 'Downloading...',
                    style: theme.textTheme.bodySmall,
                  ),
                ),
                GestureDetector(
                  onTap: () => updateService.cancelUpdate(),
                  child: Text(
                    'Cancel',
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: theme.colorScheme.primary,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                ),
              ],
            ),
          ] else ...[
            const SizedBox(height: 10),
            Row(
              children: [
                _actionButton(
                  context,
                  label: updateService.downloadUrl != null
                      ? 'Download'
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
                  _actionButton(
                    context,
                    label: 'View Release',
                    onPressed: () => updateService.openReleasePage(),
                  ),
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

  Widget _actionButton(
    BuildContext context, {
    required String label,
    required VoidCallback onPressed,
    bool primary = false,
    bool isError = false,
  }) {
    final theme = Theme.of(context);
    final color = isError ? theme.colorScheme.error : theme.colorScheme.primary;

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
