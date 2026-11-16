package com.framecorder.app.ui.common

import android.content.Context
import android.content.Intent
import android.net.Uri

const val DISCORD = "https://discord.coah80.com/?from=android"

fun Context.openDiscord() {
    runCatching { startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(DISCORD))) }
}
