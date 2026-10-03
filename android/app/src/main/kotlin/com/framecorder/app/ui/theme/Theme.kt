package com.framecorder.app.ui.theme

import android.os.Build
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MotionScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

/** framecorder's own dark scheme, built around the brand mauve (#cba6f7). */
val FramecorderDark: ColorScheme = darkColorScheme(
    primary = Color(0xFFD4BBFF),
    onPrimary = Color(0xFF3A1C6A),
    primaryContainer = Color(0xFF553A86),
    onPrimaryContainer = Color(0xFFEEDCFF),
    inversePrimary = Color(0xFF6D4FA0),
    secondary = Color(0xFFCEC2DB),
    onSecondary = Color(0xFF352D40),
    secondaryContainer = Color(0xFF4A4458),
    onSecondaryContainer = Color(0xFFE8DEF8),
    tertiary = Color(0xFFF2B8C8),
    onTertiary = Color(0xFF4A2532),
    tertiaryContainer = Color(0xFF633B48),
    onTertiaryContainer = Color(0xFFFFD9E3),
    error = Color(0xFFFFB4AB),
    onError = Color(0xFF690005),
    errorContainer = Color(0xFF8C1D18),
    onErrorContainer = Color(0xFFF9DEDC),
    background = Color(0xFF141218),
    onBackground = Color(0xFFE6E0E9),
    surface = Color(0xFF141218),
    onSurface = Color(0xFFE6E0E9),
    surfaceVariant = Color(0xFF49454F),
    onSurfaceVariant = Color(0xFFCAC4D0),
    surfaceTint = Color(0xFFD4BBFF),
    inverseSurface = Color(0xFFE6E0E9),
    inverseOnSurface = Color(0xFF322F35),
    outline = Color(0xFF938F99),
    outlineVariant = Color(0xFF49454F),
    scrim = Color(0xFF000000),
    surfaceBright = Color(0xFF3B383E),
    surfaceDim = Color(0xFF141218),
    surfaceContainerLowest = Color(0xFF0F0D13),
    surfaceContainerLow = Color(0xFF1D1B20),
    surfaceContainer = Color(0xFF211F26),
    surfaceContainerHigh = Color(0xFF2B2930),
    surfaceContainerHighest = Color(0xFF36343B),
)

/** Colors M3 has no role for: a connection that's fine, and recording. */
@Immutable
data class StatusColors(
    val ok: Color = Color(0xFF9DD6A3),
    val okContainer: Color = Color(0xFF1E3A22),
    val rec: Color = Color(0xFFFF8A80),
    val recContainer: Color = Color(0xFF8C1D18),
    val onRecContainer: Color = Color(0xFFFFDAD6),
)

val LocalStatusColors = staticCompositionLocalOf { StatusColors() }

/**
 * Always dark: it's mostly looking at VR footage. [wallpaper] takes the
 * colors from the phone's wallpaper (Android 12+) instead of framecorder's.
 */
@Composable
fun FramecorderTheme(wallpaper: Boolean = false, content: @Composable () -> Unit) {
    val scheme = if (wallpaper && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        dynamicDarkColorScheme(LocalContext.current)
    } else {
        FramecorderDark
    }
    CompositionLocalProvider(LocalStatusColors provides StatusColors()) {
        MaterialExpressiveTheme(
            colorScheme = scheme,
            motionScheme = MotionScheme.expressive(),
            typography = FramecorderType,
            content = content,
        )
    }
}
