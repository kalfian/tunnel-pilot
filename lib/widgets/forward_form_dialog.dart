import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../models/forward_config.dart';

class ForwardFormDialog extends StatefulWidget {
  final ForwardConfig? config;

  const ForwardFormDialog({super.key, this.config});

  @override
  State<ForwardFormDialog> createState() => _ForwardFormDialogState();
}

class _ForwardFormDialogState extends State<ForwardFormDialog> {
  final _formKey = GlobalKey<FormState>();

  late final TextEditingController _nameController;
  late final TextEditingController _sshHostController;
  late final TextEditingController _sshPortController;
  late final TextEditingController _sshUsernameController;
  late final TextEditingController _sshPasswordController;
  late final TextEditingController _identityFileController;
  late final TextEditingController _localBindController;
  late final TextEditingController _localPortController;
  late final TextEditingController _remoteHostController;
  late final TextEditingController _remotePortController;

  bool _useIdentityFile = false;

  @override
  void initState() {
    super.initState();
    final c = widget.config;
    _nameController = TextEditingController(text: c?.name ?? '');
    _sshHostController = TextEditingController(text: c?.sshHost ?? '');
    _sshPortController = TextEditingController(text: '${c?.sshPort ?? 22}');
    _sshUsernameController = TextEditingController(text: c?.sshUsername ?? '');
    _sshPasswordController = TextEditingController(text: c?.sshPassword ?? '');
    _identityFileController =
        TextEditingController(text: c?.identityFilePath ?? '');
    _localBindController =
        TextEditingController(text: c?.localBindAddress ?? '127.0.0.1');
    _localPortController =
        TextEditingController(text: c != null ? '${c.localPort}' : '');
    _remoteHostController = TextEditingController(text: c?.remoteHost ?? '');
    _remotePortController =
        TextEditingController(text: c != null ? '${c.remotePort}' : '');

    _useIdentityFile =
        c?.identityFilePath != null && c!.identityFilePath!.isNotEmpty;
  }

  @override
  void dispose() {
    _nameController.dispose();
    _sshHostController.dispose();
    _sshPortController.dispose();
    _sshUsernameController.dispose();
    _sshPasswordController.dispose();
    _identityFileController.dispose();
    _localBindController.dispose();
    _localPortController.dispose();
    _remoteHostController.dispose();
    _remotePortController.dispose();
    super.dispose();
  }

  Future<void> _pickIdentityFile() async {
    final result = await FilePicker.platform.pickFiles(
      dialogTitle: 'Select SSH Identity File',
      type: FileType.any,
    );
    if (result != null && result.files.single.path != null) {
      _identityFileController.text = result.files.single.path!;
    }
  }

  void _submit() {
    if (!_formKey.currentState!.validate()) return;

    final config = ForwardConfig(
      id: widget.config?.id,
      name: _nameController.text.trim(),
      sshHost: _sshHostController.text.trim(),
      sshPort: int.parse(_sshPortController.text.trim()),
      sshUsername: _sshUsernameController.text.trim(),
      sshPassword: _useIdentityFile ? null : _sshPasswordController.text,
      identityFilePath:
          _useIdentityFile ? _identityFileController.text.trim() : null,
      localBindAddress: _localBindController.text.trim(),
      localPort: int.parse(_localPortController.text.trim()),
      remoteHost: _remoteHostController.text.trim(),
      remotePort: int.parse(_remotePortController.text.trim()),
    );

    Navigator.of(context).pop(config);
  }

  String? _required(String? v) =>
      v == null || v.trim().isEmpty ? 'Required' : null;

  @override
  Widget build(BuildContext context) {
    final isEditing = widget.config != null;
    final theme = Theme.of(context);

    return Dialog(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520),
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Form(
            key: _formKey,
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  // Header
                  Row(
                    children: [
                      Container(
                        width: 32,
                        height: 32,
                        decoration: BoxDecoration(
                          color: theme.colorScheme.primary.withValues(alpha: 0.1),
                          borderRadius: BorderRadius.circular(8),
                        ),
                        child: Icon(
                          isEditing ? Icons.edit_rounded : Icons.add_rounded,
                          size: 18,
                          color: theme.colorScheme.primary,
                        ),
                      ),
                      const SizedBox(width: 12),
                      Text(
                        isEditing ? 'Edit Tunnel' : 'New Tunnel',
                        style: theme.textTheme.titleLarge,
                      ),
                    ],
                  ),

                  const SizedBox(height: 24),

                  // Name field
                  _fieldLabel('Name'),
                  const SizedBox(height: 6),
                  TextFormField(
                    controller: _nameController,
                    decoration: const InputDecoration(
                      hintText: 'e.g. Production DB',
                    ),
                    validator: _required,
                    autofocus: true,
                  ),

                  const SizedBox(height: 20),

                  // SSH Server group
                  _groupCard(
                    title: 'SSH Server',
                    icon: Icons.dns_outlined,
                    children: [
                      Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Expanded(
                            flex: 3,
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                _fieldLabel('Host'),
                                const SizedBox(height: 6),
                                TextFormField(
                                  controller: _sshHostController,
                                  decoration: const InputDecoration(
                                    hintText: 'example.com',
                                  ),
                                  validator: _required,
                                ),
                              ],
                            ),
                          ),
                          const SizedBox(width: 12),
                          SizedBox(
                            width: 80,
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                _fieldLabel('Port'),
                                const SizedBox(height: 6),
                                TextFormField(
                                  controller: _sshPortController,
                                  decoration: const InputDecoration(
                                    hintText: '22',
                                  ),
                                  keyboardType: TextInputType.number,
                                  inputFormatters: [
                                    FilteringTextInputFormatter.digitsOnly
                                  ],
                                  validator: _required,
                                ),
                              ],
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 12),
                      _fieldLabel('Username'),
                      const SizedBox(height: 6),
                      TextFormField(
                        controller: _sshUsernameController,
                        decoration: const InputDecoration(
                          hintText: 'root',
                        ),
                        validator: _required,
                      ),
                    ],
                  ),

                  const SizedBox(height: 16),

                  // Authentication group
                  _groupCard(
                    title: 'Authentication',
                    icon: Icons.lock_outline_rounded,
                    children: [
                      // Auth type selector
                      Row(
                        children: [
                          _authTab('Password', !_useIdentityFile, () {
                            setState(() => _useIdentityFile = false);
                          }),
                          const SizedBox(width: 8),
                          _authTab('Identity File', _useIdentityFile, () {
                            setState(() => _useIdentityFile = true);
                          }),
                        ],
                      ),
                      const SizedBox(height: 12),
                      if (_useIdentityFile) ...[
                        _fieldLabel('File Path'),
                        const SizedBox(height: 6),
                        Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Expanded(
                              child: TextFormField(
                                controller: _identityFileController,
                                decoration: const InputDecoration(
                                  hintText: '~/.ssh/id_rsa',
                                ),
                                validator: _useIdentityFile ? _required : null,
                              ),
                            ),
                            const SizedBox(width: 8),
                            SizedBox(
                              height: 38,
                              child: OutlinedButton(
                                onPressed: _pickIdentityFile,
                                child: const Text('Browse'),
                              ),
                            ),
                          ],
                        ),
                      ] else ...[
                        _fieldLabel('Password'),
                        const SizedBox(height: 6),
                        TextFormField(
                          controller: _sshPasswordController,
                          decoration: const InputDecoration(
                            hintText: 'Enter password',
                          ),
                          obscureText: true,
                        ),
                      ],
                    ],
                  ),

                  const SizedBox(height: 16),

                  // Port Forwarding group
                  _groupCard(
                    title: 'Port Forwarding',
                    icon: Icons.swap_horiz_rounded,
                    children: [
                      Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          // Local
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text('LOCAL',
                                    style: theme.textTheme.labelSmall),
                                const SizedBox(height: 8),
                                _fieldLabel('Address'),
                                const SizedBox(height: 6),
                                TextFormField(
                                  controller: _localBindController,
                                  decoration: const InputDecoration(
                                    hintText: '127.0.0.1',
                                  ),
                                  validator: _required,
                                ),
                                const SizedBox(height: 8),
                                _fieldLabel('Port'),
                                const SizedBox(height: 6),
                                TextFormField(
                                  controller: _localPortController,
                                  decoration: const InputDecoration(
                                    hintText: '3306',
                                  ),
                                  keyboardType: TextInputType.number,
                                  inputFormatters: [
                                    FilteringTextInputFormatter.digitsOnly
                                  ],
                                  validator: _required,
                                ),
                              ],
                            ),
                          ),

                          // Arrow
                          Padding(
                            padding: const EdgeInsets.only(top: 40),
                            child: SizedBox(
                              width: 40,
                              child: Icon(
                                Icons.arrow_forward_rounded,
                                color: theme.colorScheme.outline,
                                size: 20,
                              ),
                            ),
                          ),

                          // Remote
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text('REMOTE',
                                    style: theme.textTheme.labelSmall),
                                const SizedBox(height: 8),
                                _fieldLabel('Address'),
                                const SizedBox(height: 6),
                                TextFormField(
                                  controller: _remoteHostController,
                                  decoration: const InputDecoration(
                                    hintText: 'localhost',
                                  ),
                                  validator: _required,
                                ),
                                const SizedBox(height: 8),
                                _fieldLabel('Port'),
                                const SizedBox(height: 6),
                                TextFormField(
                                  controller: _remotePortController,
                                  decoration: const InputDecoration(
                                    hintText: '3306',
                                  ),
                                  keyboardType: TextInputType.number,
                                  inputFormatters: [
                                    FilteringTextInputFormatter.digitsOnly
                                  ],
                                  validator: _required,
                                ),
                              ],
                            ),
                          ),
                        ],
                      ),
                    ],
                  ),

                  const SizedBox(height: 24),

                  // Actions
                  Row(
                    mainAxisAlignment: MainAxisAlignment.end,
                    children: [
                      TextButton(
                        onPressed: () => Navigator.of(context).pop(),
                        child: const Text('Cancel'),
                      ),
                      const SizedBox(width: 8),
                      FilledButton(
                        onPressed: _submit,
                        child: Text(isEditing ? 'Save Changes' : 'Add Tunnel'),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  Widget _fieldLabel(String text) {
    return Text(
      text,
      style: TextStyle(
        fontSize: 12,
        fontWeight: FontWeight.w500,
        color: Theme.of(context).colorScheme.onSurface,
      ),
    );
  }

  Widget _groupCard({
    required String title,
    required IconData icon,
    required List<Widget> children,
  }) {
    final theme = Theme.of(context);
    final isDark = theme.brightness == Brightness.dark;

    return Container(
      decoration: BoxDecoration(
        color: isDark
            ? theme.colorScheme.surfaceContainerHighest
            : theme.scaffoldBackgroundColor,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: theme.dividerColor),
      ),
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(icon, size: 15, color: theme.colorScheme.outline),
              const SizedBox(width: 6),
              Text(
                title,
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.w600,
                  color: theme.colorScheme.outline,
                ),
              ),
            ],
          ),
          const SizedBox(height: 12),
          ...children,
        ],
      ),
    );
  }

  Widget _authTab(String label, bool isSelected, VoidCallback onTap) {
    final theme = Theme.of(context);

    return GestureDetector(
      onTap: onTap,
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 150),
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        decoration: BoxDecoration(
          color: isSelected
              ? theme.colorScheme.primary.withValues(alpha: 0.1)
              : Colors.transparent,
          borderRadius: BorderRadius.circular(6),
          border: Border.all(
            color: isSelected
                ? theme.colorScheme.primary.withValues(alpha: 0.4)
                : theme.dividerColor,
          ),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 12,
            fontWeight: isSelected ? FontWeight.w600 : FontWeight.w400,
            color: isSelected
                ? theme.colorScheme.primary
                : theme.colorScheme.outline,
          ),
        ),
      ),
    );
  }
}
