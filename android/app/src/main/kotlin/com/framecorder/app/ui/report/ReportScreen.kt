package com.framecorder.app.ui.report

import android.content.Intent
import android.net.Uri
import android.widget.Toast
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.framecorder.app.sync.SyncHub
import com.framecorder.app.sync.reason
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.ShapeIcon
import com.framecorder.app.ui.common.segmentShape
import com.framecorder.app.ui.theme.DataStyle
import com.framecorder.app.ui.theme.LocalStatusColors
import com.framecorder.core.CoreException
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/** Where a report is: typed, being gathered, ready to check, on its way, open on GitHub, or stuck. */
private sealed interface Step {
    data object Write : Step
    data object Collecting : Step
    data class Review(val json: String, val summary: ReportSummary) : Step
    data class Sending(val json: String, val summary: ReportSummary) : Step
    data class Sent(val number: ULong, val url: String) : Step
    data class Failed(val json: String, val summary: ReportSummary, val why: String) : Step
}

/** The pages a report moves through; checking, sending and failing all show the same page. */
private enum class Page { Write, Collecting, Check, Sent }

private val Step.page: Page
    get() = when (this) {
        is Step.Write -> Page.Write
        is Step.Collecting -> Page.Collecting
        is Step.Sent -> Page.Sent
        else -> Page.Check
    }

/**
 * Report an issue: say what went wrong, see what goes with it, send it,
 * and get the link to the GitHub issue the bot opened. What's shown at
 * the check step is exactly what's sent; nothing is gathered again after.
 */
@Composable
fun ReportScreen(hub: SyncHub, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val haptics = LocalHapticFeedback.current
    val frames by hub.frames.collectAsStateWithLifecycle()
    var description by rememberSaveable { mutableStateOf("") }
    var step by remember { mutableStateOf<Step>(Step.Write) }
    var showingAll by rememberSaveable { mutableStateOf(false) }
    var collecting by remember { mutableStateOf<Job?>(null) }

    fun collect() {
        step = Step.Collecting
        collecting = scope.launch {
            try {
                val json = hub.reportPreview(description.trim())
                step = Step.Review(json, ReportSummary.from(json))
            } catch (e: CoreException) {
                step = Step.Write
                Toast.makeText(context, "couldn't get the report ready: ${e.reason()}", Toast.LENGTH_LONG).show()
            }
        }
    }

    fun send(json: String, summary: ReportSummary) {
        step = Step.Sending(json, summary)
        haptics.performHapticFeedback(HapticFeedbackType.Confirm)
        scope.launch {
            step = try {
                val sent = hub.reportSend(json)
                Step.Sent(sent.number, sent.url)
            } catch (e: CoreException) {
                haptics.performHapticFeedback(HapticFeedbackType.Reject)
                Step.Failed(json, summary, e.reason())
            }
        }
    }

    val scheme = MaterialTheme.colorScheme
    val current = step
    Box(Modifier.fillMaxSize().background(scheme.surface)) {
        Column(
            Modifier.fillMaxSize().statusBarsPadding().verticalScroll(rememberScrollState()).padding(bottom = 140.dp),
        ) {
            Row(Modifier.height(64.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onBack) { Icon(FcIcons.Back, contentDescription = "back") }
                Text("report an issue", style = MaterialTheme.typography.titleLarge.copy(fontFamily = MaterialTheme.typography.bodyLarge.fontFamily))
            }
            Column(Modifier.padding(horizontal = 24.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
                val sent = current is Step.Sent
                ShapeIcon(
                    if (sent) FcIcons.Check else FcIcons.Bug,
                    shape = MaterialShapes.Cookie9Sided,
                    size = 72.dp,
                    iconSize = 30.dp,
                    container = if (sent) LocalStatusColors.current.okContainer else scheme.primaryContainer,
                    content = if (sent) LocalStatusColors.current.ok else scheme.onPrimaryContainer,
                )
                Text(
                    when (current) {
                        is Step.Sent -> "it's in, thanks"
                        is Step.Write, Step.Collecting -> "what went wrong?"
                        else -> "check what gets sent"
                    },
                    style = MaterialTheme.typography.headlineMedium,
                )
                Text(
                    when (current) {
                        is Step.Sent -> "issue #${current.number} is open on github. the people making framecorder see it there, and that's where any reply lands."
                        is Step.Write, Step.Collecting -> "say what happened and what you expected instead. the details needed to look into it go along, and you see them before anything is sent."
                        else -> "this is everything that goes on the issue. it's public on github, so look it over first."
                    },
                    style = MaterialTheme.typography.bodyMedium,
                    color = scheme.onSurfaceVariant,
                )
            }

            AnimatedContent(current.page, transitionSpec = { fadeIn() togetherWith fadeOut() }, label = "report step") { page ->
                Column(Modifier.padding(horizontal = 16.dp).padding(top = 8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    when (page) {
                        Page.Write -> Description(description, onChange = { description = it })
                        Page.Collecting -> Gathering(frames.firstOrNull()?.name)
                        Page.Sent -> {}
                        Page.Check -> {
                            val (summary, why) = when (current) {
                                is Step.Review -> current.summary to null
                                is Step.Sending -> current.summary to null
                                is Step.Failed -> current.summary to current.why
                                else -> return@Column
                            }
                            Quote(description.trim())
                            Spacer(Modifier.height(12.dp))
                            summary.rows.forEachIndexed { i, (label, value) -> SummaryRow(i, summary.rows.size, label, value) }
                            Spacer(Modifier.height(12.dp))
                            PublicNote(onSeeAll = { showingAll = true })
                            if (why != null) {
                                Spacer(Modifier.height(12.dp))
                                Trouble(why)
                            }
                        }
                    }
                }
            }
        }

        Column(Modifier.align(Alignment.BottomCenter).fillMaxWidth().imePadding()) {
            Spacer(
                Modifier.fillMaxWidth().height(40.dp).background(
                    Brush.verticalGradient(listOf(Color.Transparent, scheme.surface)),
                ),
            )
            Row(
                Modifier
                    .fillMaxWidth()
                    .background(scheme.surface)
                    .navigationBarsPadding()
                    .padding(start = 16.dp, end = 16.dp, bottom = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                when (current) {
                    is Step.Write -> {
                        Button(onClick = ::collect, enabled = description.isNotBlank(), modifier = Modifier.weight(1f).height(56.dp)) {
                            Text("next, see what gets sent", maxLines = 1)
                        }
                    }
                    is Step.Collecting -> {
                        OutlinedButton(
                            onClick = {
                                collecting?.cancel()
                                step = Step.Write
                            },
                            modifier = Modifier.weight(1f).height(56.dp),
                        ) { Text("cancel", maxLines = 1) }
                    }
                    is Step.Review -> {
                        OutlinedButton(onClick = { step = Step.Write }, modifier = Modifier.weight(1f).height(56.dp)) { Text("edit", maxLines = 1) }
                        Button(onClick = { send(current.json, current.summary) }, modifier = Modifier.weight(1f).height(56.dp)) { Text("send", maxLines = 1) }
                    }
                    is Step.Sending -> {
                        OutlinedButton(onClick = {}, enabled = false, modifier = Modifier.weight(1f).height(56.dp)) { Text("edit", maxLines = 1) }
                        Button(onClick = {}, enabled = false, modifier = Modifier.weight(1f).height(56.dp)) {
                            LoadingIndicator(Modifier.size(28.dp), color = scheme.onSurface.copy(alpha = 0.6f))
                        }
                    }
                    is Step.Failed -> {
                        OutlinedButton(onClick = { step = Step.Write }, modifier = Modifier.weight(1f).height(56.dp)) { Text("edit", maxLines = 1) }
                        Button(onClick = { send(current.json, current.summary) }, modifier = Modifier.weight(1f).height(56.dp)) { Text("try again", maxLines = 1) }
                    }
                    is Step.Sent -> {
                        OutlinedButton(onClick = onBack, modifier = Modifier.weight(1f).height(56.dp)) { Text("done", maxLines = 1) }
                        Button(
                            onClick = { runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(current.url))) } },
                            modifier = Modifier.weight(1f).height(56.dp),
                        ) {
                            Icon(FcIcons.Open, contentDescription = null, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.size(8.dp))
                            Text("open on github", maxLines = 1)
                        }
                    }
                }
            }
        }

        val json = when (current) {
            is Step.Review -> current.json
            is Step.Sending -> current.json
            is Step.Failed -> current.json
            else -> null
        }
        if (showingAll && json != null) {
            Everything(json, onDismiss = { showingAll = false })
        }
    }
}

@Composable
private fun Description(text: String, onChange: (String) -> Unit) {
    val scheme = MaterialTheme.colorScheme
    OutlinedTextField(
        value = text,
        onValueChange = { if (it.length <= DESCRIPTION_LIMIT) onChange(it) },
        modifier = Modifier.fillMaxWidth(),
        minLines = 6,
        placeholder = { Text("what happened, what you were doing, and what you expected", color = scheme.onSurfaceVariant.copy(alpha = 0.7f)) },
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
        shape = RoundedCornerShape(20.dp),
        supportingText = if (text.length >= DESCRIPTION_LIMIT - 500) {
            { Text("${text.length} / $DESCRIPTION_LIMIT", style = DataStyle) }
        } else {
            null
        },
    )
}

@Composable
private fun Gathering(frame: String?) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(20.dp))
            .background(MaterialTheme.colorScheme.surfaceContainer)
            .padding(16.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        LoadingIndicator(Modifier.size(32.dp))
        Text(
            if (frame != null) "gathering details from this phone and $frame. a few seconds." else "gathering details from this phone.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/** What they typed, the way it will head the issue. */
@Composable
private fun Quote(text: String) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(20.dp)).background(MaterialTheme.colorScheme.surfaceContainerLow).padding(16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(Modifier.size(width = 3.dp, height = 20.dp).clip(RoundedCornerShape(2.dp)).background(MaterialTheme.colorScheme.primary))
        Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurface)
    }
}

@Composable
private fun SummaryRow(index: Int, count: Int, label: String, value: String) {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(segmentShape(index, count, outer = 20.dp))
            .background(MaterialTheme.colorScheme.surfaceContainer)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
        Text(value, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun PublicNote(onSeeAll: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(20.dp)).background(scheme.surfaceContainerLow).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Icon(FcIcons.Info, contentDescription = null, tint = scheme.primary, modifier = Modifier.size(20.dp))
            Text(
                "this opens a public issue on github.com/coah80/framecorder, posted by cobby-bot. names, logins, pairing keys and addresses are taken out first.",
                style = MaterialTheme.typography.bodyMedium,
                color = scheme.onSurfaceVariant,
            )
        }
        OutlinedButton(onClick = onSeeAll, modifier = Modifier.fillMaxWidth()) { Text("see everything that's sent") }
    }
}

@Composable
private fun Trouble(why: String) {
    val scheme = MaterialTheme.colorScheme
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(20.dp)).background(scheme.errorContainer).padding(16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Icon(FcIcons.Close, contentDescription = null, tint = scheme.onErrorContainer, modifier = Modifier.size(20.dp))
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text("it didn't go through", style = MaterialTheme.typography.titleSmall, color = scheme.onErrorContainer)
            Text(why, style = MaterialTheme.typography.bodyMedium, color = scheme.onErrorContainer)
        }
    }
}

/** The whole report as it's sent, a line per row so even the logs scroll smoothly. */
@Composable
private fun Everything(json: String, onDismiss: () -> Unit) {
    val lines = remember(json) { json.lines() }
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = MaterialTheme.colorScheme.surfaceContainerLow) {
        Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 24.dp)) {
            Text("everything that's sent", style = MaterialTheme.typography.headlineSmall, modifier = Modifier.padding(start = 8.dp, bottom = 4.dp))
            Text(
                "${lines.size} lines of json, exactly as the issue gets them.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 8.dp, bottom = 14.dp),
            )
            // one sideways scroll shared by every line, so long log lines pan together
            val sideways = rememberScrollState()
            LazyColumn(
                Modifier
                    .fillMaxWidth()
                    .height(440.dp)
                    .clip(RoundedCornerShape(20.dp))
                    .background(MaterialTheme.colorScheme.surfaceContainerLowest),
                contentPadding = PaddingValues(12.dp),
            ) {
                items(lines) { line ->
                    Text(
                        line,
                        fontFamily = FontFamily.Monospace,
                        fontSize = 11.sp,
                        lineHeight = 15.sp,
                        maxLines = 1,
                        softWrap = false,
                        modifier = Modifier.horizontalScroll(sideways),
                    )
                }
            }
        }
    }
}
