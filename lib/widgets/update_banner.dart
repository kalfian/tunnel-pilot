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
        color: theme.colorScheme.primary.withValues(alpha: 0.08),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(
          color: theme.colorScheme.primary.withValues(alpha: 0.2),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(
                Icons.system_update_outlined,
                size: 18,
                color: theme.colorScheme.primary,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  'Update available: v${updateService.latestVersion}',
                  style: theme.textTheme.bodyMedium?.copyWith(
                    fontWeight: FontWeight.w600,
                    color: theme.colorScheme.primary,
                  ),
                ),
              ),
            ],
          ),
          if (updateService.isDownloading) ...[
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
            Text(
              '${(updateService.downloadProgress * 100).toInt()}%',
              style: theme.textTheme.bodySmall,
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
  }) {
    final theme = Theme.of(context);

    return GestureDetector(
      onTap: onPressed,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        decoration: BoxDecoration(
          color: primary
              ? theme.colorScheme.primary
              : theme.colorScheme.primary.withValues(alpha: 0.1),
          borderRadius: BorderRadius.circular(6),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 12,
            fontWeight: FontWeight.w500,
            color: primary ? Colors.white : theme.colorScheme.primary,
          ),
        ),
      ),
    );
  }
}
