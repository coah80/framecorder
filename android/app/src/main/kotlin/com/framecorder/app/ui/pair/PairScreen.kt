package com.framecorder.app.ui.pair

import android.content.ClipboardManager
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import com.framecorder.app.sync.SyncHub
import com.framecorder.app.sync.reason
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.dotted
import com.framecorder.app.ui.common.host
import com.framecorder.app.ui.common.segmentShape
import com.framecorder.app.ui.common.shortPrint
import com.framecorder.app.ui.theme.DataStyle
import com.framecorder.core.CoreException
import com.framecorder.core.FoundFrame
import com.framecorder.core.FrameStatus
import com.framecorder.core.parsePairLink
import com.framecorder.core.sweepNetworks
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

private const val LOCAL_NETWORK = "android.permission.ACCESS_LOCAL_NETWORK"
private const val ANDROID_17 = 37

private enum class How(val text: String) { Name("found by name"), Direct("found by asking directly"), Typed("typed in") }

/** Pairing on its own screen, for another frame or pairing again. */
@Composable
fun PairScreen(hub: SyncHub, link: String?, replaces: String?, onBack: () -> Unit, onPaired: (FrameStatus) -> Unit) {
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surface).statusBarsPadding()) {
        Box(Modifier.height(56.dp).padding(horizontal = 4.dp), contentAlignment = Alignment.CenterStart) {
            IconButton(onClick = onBack) { Icon(FcIcons.Back, contentDescription = "back") }
        }
        Pairing(
            hub = hub,
            link = link,
            replaces = replaces,
            title = if (replaces != null) "pair it again" else "pair a frame",
            onPaired = onPaired,
            modifier = Modifier.weight(1f),
            bottom = 24.dp,
        )
    }
}

/**
 * Everything pairing takes: the QR code, frames found on this Wi-Fi, the
 * code. The frame tab shows it whole when nothing's paired yet.
 */
@Composable
fun Pairing(
    hub: SyncHub,
    modifier: Modifier = Modifier,
    link: String? = null,
    /** A paired frame this one stands for (it got a new certificate, or forgot us). */
    replaces: String? = null,
    title: String = "pair your frame",
    bottom: Dp = 140.dp,
    onPaired: (FrameStatus) -> Unit = {},
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val found = remember { mutableStateMapOf<String, FoundFrame>() }
    val how = remember { mutableStateMapOf<String, How>() }
    var selected by rememberSaveable { mutableStateOf<String?>(null) }
    var code by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var typing by rememberSaveable { mutableStateOf(false) }
    val codeFocus = remember { FocusRequester() }
    val codeInView = remember { BringIntoViewRequester() }
    val networks = remember { runCatching { sweepNetworks() }.getOrDefault(emptyList()) }

    fun allowed() = Build.VERSION.SDK_INT < ANDROID_17 ||
        ContextCompat.checkSelfPermission(context, LOCAL_NETWORK) == PackageManager.PERMISSION_GRANTED
    var localNetwork by remember { mutableStateOf(allowed()) }
    val askLocalNetwork = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { localNetwork = it }

    var pairingWith by remember { mutableStateOf<String?>(null) }

    fun pairWith(name: String?, what: suspend () -> FrameStatus) {
        if (busy) return
        busy = true
        error = null
        pairingWith = name
        scope.launch {
            try {
                onPaired(what())
            } catch (e: CoreException) {
                error = e.reason()
                code = ""
            } finally {
                busy = false
            }
        }
    }

    fun add(frame: FoundFrame, by: How) {
        found[frame.fingerprint] = frame
        if (how[frame.fingerprint] == null || by == How.Typed) how[frame.fingerprint] = by
        if (selected == null && found.size == 1) selected = frame.fingerprint
    }

    LaunchedEffect(link) { if (link != null) pairWith(linkName(link)) { hub.pairLink(link, replaces) } }

    LaunchedEffect(localNetwork) {
        if (!localNetwork) return@LaunchedEffect
        launch { hub.finder.browse().collect { add(it, How.Name) } }
        launch {
            while (true) {
                hub.sweep(8).forEach { add(it, How.Direct) }
                delay(12_000)
            }
        }
    }

    val keyboard = WindowInsets.isImeVisible
    LaunchedEffect(selected) { if (selected != null) runCatching { codeFocus.requestFocus() } }
    LaunchedEffect(selected, keyboard) {
        if (selected != null) {
            delay(250)
            codeInView.bringIntoView()
        }
    }

    LaunchedEffect(code, selected) {
        val frame = selected?.let { found[it] }
        if (code.length == 6 && frame != null) pairWith(frame.name) { hub.pair(frame.addr, frame.fingerprint, code, replaces) }
    }

    Column(
        modifier
            .fillMaxWidth()
            .imePadding()
            .verticalScroll(rememberScrollState())
            .navigationBarsPadding()
            .padding(bottom = bottom),
    ) {
        Hero(searching = found.isEmpty() && localNetwork, title = title)

        if (!localNetwork) {
            Note(
                title = "let framecorder see your wi-fi",
                text = "android asks before an app can talk to devices on your network. framecorder only talks to your frame.",
                action = "allow",
                onAction = { askLocalNetwork.launch(LOCAL_NETWORK) },
            )
        }

        Column(Modifier.padding(horizontal = 16.dp).padding(top = 28.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Step(0, "1", "on your frame") {
                Text(
                    buildAnnotatedString {
                        append("framecorder tab, settings, sync, ")
                        withStyle(SpanStyle(color = MaterialTheme.colorScheme.onSurface, fontWeight = FontWeight.Medium)) { append("pair a device") }
                        append(". the code keeps working for 10 minutes, even with the headset off.")
                    },
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Step(1, "2", "point this phone at it") {
                Button(
                    onClick = { scan(context, onLink = { pairWith(linkName(it)) { hub.pairLink(it, replaces) } }, onError = { error = it }) },
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth().padding(top = 6.dp).height(56.dp),
                    contentPadding = ButtonDefaults.contentPaddingFor(56.dp),
                ) {
                    Icon(FcIcons.Qr, contentDescription = null)
                    Spacer(Modifier.width(10.dp))
                    Text("scan the qr code", style = MaterialTheme.typography.titleMedium)
                }
            }
        }

        Text(
            "or pick it from this wi-fi",
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(start = 24.dp, top = 24.dp, bottom = 10.dp),
        )
        Column(Modifier.padding(horizontal = 16.dp).selectableGroup().animateContentSize(), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            val list = found.values.sortedBy { it.name }
            if (list.isEmpty()) Searching(networks, localNetwork)
            list.forEachIndexed { i, f ->
                FoundRow(f, how[f.fingerprint] ?: How.Name, selected == f.fingerprint, i, list.size) {
                    selected = f.fingerprint
                    error = null
                }
            }
        }

        AnimatedVisibility(selected != null) {
            val frame = selected?.let { found[it] }
            Column(
                Modifier.bringIntoViewRequester(codeInView).padding(horizontal = 24.dp).padding(top = 24.dp, bottom = 48.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text("type the code it shows", style = MaterialTheme.typography.titleMedium)
                CodeField(code = code, onChange = { code = it; error = null }, enabled = !busy, focus = codeFocus)
                if (frame != null) {
                    Text("id ${shortPrint(frame.fingerprint)}", style = DataStyle, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }

        Row(
            Modifier.padding(horizontal = 24.dp, vertical = 12.dp).animateContentSize(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            when {
                busy -> {
                    LoadingIndicator(Modifier.size(32.dp))
                    Text(pairingWith?.let { "pairing with $it..." } ?: "pairing...", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary)
                }
                error != null -> Text(error!!, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)
                selected != null -> Text(
                    "pairs as soon as you type the last digit.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = { typing = true }, modifier = Modifier.weight(1f).height(48.dp), enabled = !busy) {
                Icon(FcIcons.Keyboard, contentDescription = null, modifier = Modifier.size(20.dp))
                Spacer(Modifier.width(8.dp))
                Text("type its address", maxLines = 1)
            }
            OutlinedButton(
                onClick = {
                    val text = clipboardText(context)
                    if (text != null && text.trim().startsWith("framecorder://")) pairWith(linkName(text)) { hub.pairLink(text, replaces) }
                    else error = "copy the framecorder://pair link first, then paste it here."
                },
                modifier = Modifier.weight(1f).height(48.dp),
                enabled = !busy,
            ) {
                Icon(FcIcons.Link, contentDescription = null, modifier = Modifier.size(20.dp))
                Spacer(Modifier.width(8.dp))
                Text("paste a link", maxLines = 1)
            }
        }
    }

    if (typing) {
        AddressDialog(
            onDismiss = { typing = false },
            onAddress = { addr ->
                typing = false
                scope.launch {
                    try {
                        val frame = hub.identify(addr)
                        add(frame, How.Typed)
                        selected = frame.fingerprint
                    } catch (e: CoreException) {
                        error = e.reason()
                    }
                }
            },
        )
    }
}

/** A shape that keeps morphing while it looks, and settles once something's there. */
@Composable
private fun Hero(searching: Boolean, title: String) {
    Column(Modifier.fillMaxWidth().padding(top = 32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Box(Modifier.size(156.dp), contentAlignment = Alignment.Center) {
            AnimatedContent(
                targetState = searching,
                transitionSpec = { (fadeIn() + scaleIn(initialScale = 0.8f)) togetherWith (fadeOut() + scaleOut(targetScale = 0.8f)) },
                label = "hero",
            ) { looking ->
                if (looking) {
                    LoadingIndicator(Modifier.size(156.dp), color = MaterialTheme.colorScheme.primaryContainer)
                } else {
                    Box(Modifier.size(132.dp).clip(MaterialShapes.Cookie9Sided.toShape()).background(MaterialTheme.colorScheme.primaryContainer))
                }
            }
            Icon(FcIcons.Headset, contentDescription = null, tint = MaterialTheme.colorScheme.onPrimaryContainer, modifier = Modifier.size(52.dp))
        }
        Text(
            dotted(title),
            style = MaterialTheme.typography.displaySmall,
            modifier = Modifier.padding(top = 24.dp, start = 24.dp, end = 24.dp),
        )
        Text(
            "clips come straight to this phone over your wi-fi. nothing goes through the internet.",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 8.dp, start = 32.dp, end = 32.dp),
        )
    }
}

@Composable
private fun Step(index: Int, number: String, title: String, content: @Composable () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(segmentShape(index, 2))
            .background(MaterialTheme.colorScheme.surfaceContainer)
            .padding(horizontal = 18.dp, vertical = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Box(
            Modifier.size(28.dp).clip(CircleShape).background(MaterialTheme.colorScheme.secondaryContainer),
            contentAlignment = Alignment.Center,
        ) {
            Text(number, style = DataStyle, color = MaterialTheme.colorScheme.onSecondaryContainer)
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(title, style = MaterialTheme.typography.titleMedium)
            content()
        }
    }
}

@Composable
private fun Searching(networks: List<String>, allowed: Boolean) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(24.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerLow)
            .padding(horizontal = 18.dp, vertical = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        if (allowed) LoadingIndicator(Modifier.size(40.dp))
        Column(Modifier.weight(1f)) {
            Text(if (allowed) "looking for frames" else "waiting for permission", style = MaterialTheme.typography.titleMedium)
            Text(
                when {
                    !allowed -> "framecorder can't look until it's allowed to."
                    networks.isEmpty() -> "is this phone on wi-fi?"
                    else -> "asking by name, and checking ${networks.joinToString()} directly in case your router blocks that"
                },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun FoundRow(frame: FoundFrame, how: How, picked: Boolean, index: Int, count: Int, onPick: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .clip(segmentShape(index, count))
            .background(if (picked) scheme.secondaryContainer else scheme.surfaceContainer)
            .selectable(selected = picked, role = Role.RadioButton, onClick = onPick)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Box(
            Modifier.size(40.dp).clip(if (picked) RoundedCornerShape(14.dp) else CircleShape).background(if (picked) scheme.primary else scheme.surfaceContainerHighest),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                if (picked) FcIcons.Check else FcIcons.Headset,
                contentDescription = null,
                tint = if (picked) scheme.onPrimary else scheme.onSurfaceVariant,
                modifier = Modifier.size(if (picked) 24.dp else 20.dp),
            )
        }
        Column(Modifier.weight(1f)) {
            Text(
                frame.name,
                style = MaterialTheme.typography.titleMedium,
                color = if (picked) scheme.onSecondaryContainer else scheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                "${host(frame.addr)} · ${how.text}",
                style = MaterialTheme.typography.bodySmall,
                color = if (picked) scheme.onSecondaryContainer.copy(alpha = 0.85f) else scheme.onSurfaceVariant,
                maxLines = 1,
            )
        }
    }
}

@Composable
private fun Note(title: String, text: String, action: String, onAction: () -> Unit) {
    Column(
        Modifier
            .padding(start = 16.dp, end = 16.dp, top = 24.dp)
            .fillMaxWidth()
            .clip(RoundedCornerShape(24.dp))
            .background(MaterialTheme.colorScheme.tertiaryContainer)
            .padding(18.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(FcIcons.Wifi, contentDescription = null, tint = MaterialTheme.colorScheme.onTertiaryContainer)
            Text(title, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onTertiaryContainer)
        }
        Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onTertiaryContainer)
        Button(onClick = onAction) { Text(action) }
    }
}

@Composable
private fun AddressDialog(onDismiss: () -> Unit, onAddress: (String) -> Unit) {
    var addr by rememberSaveable { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(FcIcons.Keyboard, contentDescription = null) },
        title = { Text("its address") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(
                    "the frame's ip address, like 192.168.1.42. your router's list of devices shows it, and so do the frame's wi-fi settings.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OutlinedTextField(
                    value = addr,
                    onValueChange = { addr = it.trim() },
                    singleLine = true,
                    placeholder = { Text("192.168.1.42") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                    textStyle = DataStyle.copy(fontSize = MaterialTheme.typography.bodyLarge.fontSize, color = MaterialTheme.colorScheme.onSurface),
                )
            }
        },
        confirmButton = { TextButton(onClick = { onAddress(addr) }, enabled = addr.isNotBlank()) { Text("find it") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("cancel") } },
    )
}

/** Google's code scanner: the scanning screen is the system's, so no camera permission. */
private fun scan(context: Context, onLink: (String) -> Unit, onError: (String) -> Unit) {
    val options = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
    GmsBarcodeScanning.getClient(context, options)
        .startScan()
        .addOnSuccessListener { code ->
            val raw = code.rawValue
            if (raw != null && raw.startsWith("framecorder://")) onLink(raw)
            else onError("that qr code isn't from framecorder. on your frame: settings, sync, pair a device.")
        }
        .addOnFailureListener { onError("the scanner isn't available on this phone. pick your frame below and type its code instead.") }
}

/** The frame's name from a pairing link, for saying who it's pairing with. */
private fun linkName(link: String): String? = runCatching { parsePairLink(link.trim()).name }.getOrNull()?.takeIf { it.isNotBlank() }

private fun clipboardText(context: Context): String? =
    context.getSystemService(ClipboardManager::class.java)?.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text?.toString()
