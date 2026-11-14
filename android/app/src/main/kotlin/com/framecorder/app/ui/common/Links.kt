package com.framecorder.app.ui.common

import android.content.Context
import android.content.Intent
import android.net.Uri

/** The discord, from this app: the link counts the visit and passes it on to the invite. */
const val DISCORD = "https://discord.coah80.com/?from=android"

fun Context.openDiscord() {
    runCatching { startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(DISCORD))) }
}
