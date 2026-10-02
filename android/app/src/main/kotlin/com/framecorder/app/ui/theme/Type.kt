package com.framecorder.app.ui.theme

import androidx.compose.material3.Typography
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import com.framecorder.app.R

/** Big words: titles and the wordmark. */
val Montserrat = FontFamily(Font(R.font.montserrat_bold, FontWeight.Bold), Font(R.font.montserrat_bold, FontWeight.ExtraBold))

/** Everything you read. */
val Poppins = FontFamily(
    Font(R.font.poppins_regular, FontWeight.Normal),
    Font(R.font.poppins_medium, FontWeight.Medium),
    Font(R.font.poppins_medium, FontWeight.SemiBold),
)

/** Numbers: durations, sizes, codes, addresses. */
val SpaceGrotesk = FontFamily(Font(R.font.space_grotesk_medium, FontWeight.Medium))

private fun display(size: Int, line: Int, weight: FontWeight = FontWeight.ExtraBold, tracking: Double = -0.02) =
    TextStyle(fontFamily = Montserrat, fontWeight = weight, fontSize = size.sp, lineHeight = line.sp, letterSpacing = tracking.em)

private fun body(size: Int, line: Int, weight: FontWeight = FontWeight.Normal, tracking: Double = 0.0) =
    TextStyle(fontFamily = Poppins, fontWeight = weight, fontSize = size.sp, lineHeight = line.sp, letterSpacing = tracking.em)

val FramecorderType = Typography(
    displayLarge = display(57, 64),
    displayMedium = display(45, 52),
    displaySmall = display(36, 44),
    headlineLarge = display(32, 40),
    headlineMedium = display(28, 36, FontWeight.Bold, -0.01),
    headlineSmall = display(24, 32, FontWeight.Bold, -0.01),
    titleLarge = display(22, 28, FontWeight.Bold, -0.01),
    titleMedium = body(16, 24, FontWeight.Medium),
    titleSmall = body(14, 20, FontWeight.Medium),
    bodyLarge = body(16, 24),
    bodyMedium = body(14, 20),
    bodySmall = body(12, 16),
    labelLarge = body(14, 20, FontWeight.Medium),
    labelMedium = body(12, 16, FontWeight.Medium),
    labelSmall = body(11, 16, FontWeight.Medium),
    displayLargeEmphasized = display(57, 64),
    displayMediumEmphasized = display(45, 52),
    displaySmallEmphasized = display(36, 44),
    headlineLargeEmphasized = display(32, 40),
    headlineMediumEmphasized = display(28, 36),
    headlineSmallEmphasized = display(24, 32),
    titleLargeEmphasized = display(22, 28),
    titleMediumEmphasized = body(16, 24, FontWeight.SemiBold),
    titleSmallEmphasized = body(14, 20, FontWeight.SemiBold),
    bodyLargeEmphasized = body(16, 24, FontWeight.Medium),
    bodyMediumEmphasized = body(14, 20, FontWeight.Medium),
    bodySmallEmphasized = body(12, 16, FontWeight.Medium),
    labelLargeEmphasized = body(14, 20, FontWeight.SemiBold),
    labelMediumEmphasized = body(12, 16, FontWeight.SemiBold),
    labelSmallEmphasized = body(11, 16, FontWeight.SemiBold),
)

/** For numbers next to body text. */
val DataStyle = TextStyle(fontFamily = SpaceGrotesk, fontWeight = FontWeight.Medium, fontSize = 13.sp, lineHeight = 18.sp)
