import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import 'providers/app_settings_provider.dart';
import 'screens/settings_window.dart';

/// Monospace stack for technical content (ports, latency, timestamps).
/// Flutter does NOT parse comma-separated `fontFamily` strings — it needs a
/// real fallback list. Use this everywhere instead of a single CSS-style name.
const List<String> kMonoFontFallback = <String>[
  'SF Mono',
  'Menlo',
  'Consolas',
  'monospace',
];

/// Shared, intentional design tokens. Centralizes the values that were
/// previously scattered as magic numbers across widgets.
@immutable
class AppTokens extends ThemeExtension<AppTokens> {
  const AppTokens({
    required this.statusConnected,
    required this.statusPending,
    required this.statusError,
    required this.statusIdle,
    required this.hover,
    required this.selected,
  });

  // Semantic status roles (single source of truth — was duplicated as raw hex
  // in forward_list_tile and logs_section).
  final Color statusConnected; // green
  final Color statusPending; // amber
  final Color statusError; // red
  final Color statusIdle; // grey

  // Interaction surface tints.
  final Color hover;
  final Color selected;

  @override
  AppTokens copyWith({
    Color? statusConnected,
    Color? statusPending,
    Color? statusError,
    Color? statusIdle,
    Color? hover,
    Color? selected,
  }) =>
      AppTokens(
        statusConnected: statusConnected ?? this.statusConnected,
        statusPending: statusPending ?? this.statusPending,
        statusError: statusError ?? this.statusError,
        statusIdle: statusIdle ?? this.statusIdle,
        hover: hover ?? this.hover,
        selected: selected ?? this.selected,
      );

  @override
  AppTokens lerp(ThemeExtension<AppTokens>? other, double t) {
    if (other is! AppTokens) return this;
    return AppTokens(
      statusConnected: Color.lerp(statusConnected, other.statusConnected, t)!,
      statusPending: Color.lerp(statusPending, other.statusPending, t)!,
      statusError: Color.lerp(statusError, other.statusError, t)!,
      statusIdle: Color.lerp(statusIdle, other.statusIdle, t)!,
      hover: Color.lerp(hover, other.hover, t)!,
      selected: Color.lerp(selected, other.selected, t)!,
    );
  }
}

extension AppTokensX on BuildContext {
  AppTokens get tokens => Theme.of(this).extension<AppTokens>()!;
}

class TunnelPilotApp extends StatelessWidget {
  const TunnelPilotApp({super.key});

  static final ThemeData _light = _lightTheme();
  static final ThemeData _dark = _darkTheme();

  @override
  Widget build(BuildContext context) {
    final themeMode = context.watch<AppSettingsProvider>().themeModeEnum;

    return MaterialApp(
      title: 'Tunnel Pilot',
      debugShowCheckedModeBanner: false,
      theme: _light,
      darkTheme: _dark,
      themeMode: themeMode,
      home: const SettingsWindow(),
    );
  }

  static ThemeData _lightTheme() {
    const bg = Color(0xFFF8F9FB);
    const surface = Color(0xFFFFFFFF);
    const border = Color(0xFFE2E5EA);
    const textPrimary = Color(0xFF1A1D23);
    const textSecondary = Color(0xFF6B7280);
    const accent = Color(0xFF007BFF);

    return ThemeData(
      brightness: Brightness.light,
      useMaterial3: true,
      fontFamily: '.SF Pro Text',
      extensions: const <ThemeExtension<dynamic>>[
        AppTokens(
          statusConnected: Color(0xFF16A34A),
          statusPending: Color(0xFFD97706),
          statusError: Color(0xFFDC2626),
          statusIdle: Color(0xFF9CA3AF),
          hover: Color(0x0A000000), // ~4% black
          selected: Color(0x14007BFF), // accent @ 8%
        ),
      ],
      colorScheme: const ColorScheme.light(
        primary: accent,
        onPrimary: Colors.white,
        surface: surface,
        onSurface: textPrimary,
        outline: textSecondary,
        outlineVariant: border,
        error: Color(0xFFDC2626),
      ),
      scaffoldBackgroundColor: bg,
      dividerColor: border,
      splashFactory: NoSplash.splashFactory,
      textTheme: const TextTheme(
        bodySmall: TextStyle(fontSize: 12, color: textSecondary),
        bodyMedium: TextStyle(fontSize: 13, color: textPrimary),
        bodyLarge: TextStyle(fontSize: 14, color: textPrimary),
        titleSmall: TextStyle(fontSize: 13, fontWeight: FontWeight.w600, color: textPrimary),
        titleMedium: TextStyle(fontSize: 15, fontWeight: FontWeight.w600, color: textPrimary),
        titleLarge: TextStyle(fontSize: 18, fontWeight: FontWeight.w700, color: textPrimary),
        labelSmall: TextStyle(fontSize: 11, fontWeight: FontWeight.w500, color: textSecondary, letterSpacing: 0.5),
      ),
      inputDecorationTheme: InputDecorationTheme(
        filled: false,
        isDense: true,
        contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(8),
          borderSide: const BorderSide(color: border),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(8),
          borderSide: const BorderSide(color: border),
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(8),
          borderSide: const BorderSide(color: accent, width: 1.5),
        ),
        errorBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(8),
          borderSide: BorderSide(color: Colors.red.shade400),
        ),
        labelStyle: const TextStyle(fontSize: 13, color: textSecondary),
        hintStyle: const TextStyle(fontSize: 13, color: textSecondary),
        errorStyle: TextStyle(fontSize: 11, color: Colors.red.shade600),
      ),
      dialogTheme: DialogThemeData(
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
        surfaceTintColor: Colors.transparent,
        backgroundColor: surface,
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          backgroundColor: accent,
          foregroundColor: Colors.white,
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
          textStyle: const TextStyle(fontSize: 13, fontWeight: FontWeight.w600),
        ),
      ),
      textButtonTheme: TextButtonThemeData(
        style: TextButton.styleFrom(
          foregroundColor: textSecondary,
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
          textStyle: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500),
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          foregroundColor: textPrimary,
          side: const BorderSide(color: border),
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 8),
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
          textStyle: const TextStyle(fontSize: 12, fontWeight: FontWeight.w500),
        ),
      ),
    );
  }

  static ThemeData _darkTheme() {
    const bg = Color(0xFF111318);
    const surface = Color(0xFF1A1D24);
    const surfaceElevated = Color(0xFF22262F);
    const border = Color(0xFF2E333D);
    const textPrimary = Color(0xFFE5E7EB);
    const textSecondary = Color(0xFF8B919A);
    const accent = Color(0xFF3D9AFF);

    return ThemeData(
      brightness: Brightness.dark,
      useMaterial3: true,
      fontFamily: '.SF Pro Text',
      extensions: const <ThemeExtension<dynamic>>[
        AppTokens(
          statusConnected: Color(0xFF34D399),
          statusPending: Color(0xFFFBBF24),
          statusError: Color(0xFFF87171),
          statusIdle: Color(0xFF6B7280),
          hover: Color(0x0DFFFFFF), // ~5% white
          selected: Color(0x1F3D9AFF), // accent @ 12%
        ),
      ],
      colorScheme: const ColorScheme.dark(
        primary: accent,
        onPrimary: Color(0xFF111318),
        surface: surface,
        onSurface: textPrimary,
        outline: textSecondary,
        outlineVariant: border,
        surfaceContainerHighest: surfaceElevated,
        error: Color(0xFFF87171),
      ),
      scaffoldBackgroundColor: bg,
      dividerColor: border,
      splashFactory: NoSplash.splashFactory,
      textTheme: const TextTheme(
        bodySmall: TextStyle(fontSize: 12, color: textSecondary),
        bodyMedium: TextStyle(fontSize: 13, color: textPrimary),
        bodyLarge: TextStyle(fontSize: 14, color: textPrimary),
        titleSmall: TextStyle(fontSize: 13, fontWeight: FontWeight.w600, color: textPrimary),
        titleMedium: TextStyle(fontSize: 15, fontWeight: FontWeight.w600, color: textPrimary),
        titleLarge: TextStyle(fontSize: 18, fontWeight: FontWeight.w700, color: textPrimary),
        labelSmall: TextStyle(fontSize: 11, fontWeight: FontWeight.w500, color: textSecondary, letterSpacing: 0.5),
      ),
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: surfaceElevated,
        isDense: true,
        contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(8),
          borderSide: const BorderSide(color: border),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(8),
          borderSide: const BorderSide(color: border),
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(8),
          borderSide: const BorderSide(color: accent, width: 1.5),
        ),
        errorBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(8),
          borderSide: BorderSide(color: Colors.red.shade400),
        ),
        labelStyle: const TextStyle(fontSize: 13, color: textSecondary),
        hintStyle: const TextStyle(fontSize: 13, color: textSecondary),
        errorStyle: TextStyle(fontSize: 11, color: Colors.red.shade400),
      ),
      dialogTheme: DialogThemeData(
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
        surfaceTintColor: Colors.transparent,
        backgroundColor: surface,
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          backgroundColor: accent,
          foregroundColor: const Color(0xFF111318),
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
          textStyle: const TextStyle(fontSize: 13, fontWeight: FontWeight.w600),
        ),
      ),
      textButtonTheme: TextButtonThemeData(
        style: TextButton.styleFrom(
          foregroundColor: textSecondary,
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
          textStyle: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500),
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          foregroundColor: textPrimary,
          side: const BorderSide(color: border),
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 8),
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
          textStyle: const TextStyle(fontSize: 12, fontWeight: FontWeight.w500),
        ),
      ),
    );
  }
}
