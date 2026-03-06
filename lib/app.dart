import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import 'providers/app_settings_provider.dart';
import 'screens/settings_window.dart';

class TunnelPilotApp extends StatelessWidget {
  const TunnelPilotApp({super.key});

  @override
  Widget build(BuildContext context) {
    final themeMode = context.watch<AppSettingsProvider>().themeModeEnum;

    return MaterialApp(
      title: 'Tunnel Pilot',
      debugShowCheckedModeBanner: false,
      theme: _lightTheme(),
      darkTheme: _darkTheme(),
      themeMode: themeMode,
      home: const SettingsWindow(),
    );
  }

  ThemeData _lightTheme() {
    const bg = Color(0xFFF8F9FB);
    const surface = Color(0xFFFFFFFF);
    const border = Color(0xFFE2E5EA);
    const textPrimary = Color(0xFF1A1D23);
    const textSecondary = Color(0xFF6B7280);
    const accent = Color(0xFF0D9488);

    return ThemeData(
      brightness: Brightness.light,
      useMaterial3: true,
      fontFamily: '.SF Pro Text',
      colorScheme: const ColorScheme.light(
        primary: accent,
        onPrimary: Colors.white,
        surface: surface,
        onSurface: textPrimary,
        outline: textSecondary,
        outlineVariant: border,
      ),
      scaffoldBackgroundColor: bg,
      dividerColor: border,
      splashFactory: InkSplash.splashFactory,
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

  ThemeData _darkTheme() {
    const bg = Color(0xFF111318);
    const surface = Color(0xFF1A1D24);
    const surfaceElevated = Color(0xFF22262F);
    const border = Color(0xFF2E333D);
    const textPrimary = Color(0xFFE5E7EB);
    const textSecondary = Color(0xFF8B919A);
    const accent = Color(0xFF2DD4BF);

    return ThemeData(
      brightness: Brightness.dark,
      useMaterial3: true,
      fontFamily: '.SF Pro Text',
      colorScheme: const ColorScheme.dark(
        primary: accent,
        onPrimary: Color(0xFF111318),
        surface: surface,
        onSurface: textPrimary,
        outline: textSecondary,
        outlineVariant: border,
        surfaceContainerHighest: surfaceElevated,
      ),
      scaffoldBackgroundColor: bg,
      dividerColor: border,
      splashFactory: InkSplash.splashFactory,
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
