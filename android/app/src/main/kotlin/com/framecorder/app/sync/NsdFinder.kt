package com.framecorder.app.sync

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import android.util.Log
import com.framecorder.core.FoundFrame
import com.framecorder.core.Finder
import java.net.Inet4Address
import java.net.InetAddress
import java.util.concurrent.Executors
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull

private const val TAG = "framecorder"
/** What framecorder-sync advertises, minus the `.local.` NsdManager adds. */
private const val SERVICE = "_framecorder._tcp"

/**
 * Finds Frames with Android's own mDNS service. Unlike raw multicast it
 * needs no multicast lock and keeps working when the app's in the
 * background; on Android 17 it needs the local network permission.
 */
class NsdFinder(context: Context) : Finder {
    private val nsd = context.getSystemService(NsdManager::class.java)
    private val executor = Executors.newSingleThreadExecutor()

    /** Every Frame that shows up while this is collected. */
    fun browse(): Flow<FoundFrame> = callbackFlow {
        val listener = object : NsdManager.DiscoveryListener {
            override fun onServiceFound(info: NsdServiceInfo) {
                resolve(info) { trySend(it) }
            }

            override fun onServiceLost(info: NsdServiceInfo) {}
            override fun onDiscoveryStarted(serviceType: String) {}
            override fun onDiscoveryStopped(serviceType: String) {}

            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.w(TAG, "mDNS browse didn't start ($errorCode)")
                close()
            }

            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {}
        }
        try {
            nsd.discoverServices(SERVICE, NsdManager.PROTOCOL_DNS_SD, listener)
        } catch (e: SecurityException) {
            // no local network permission (Android 17)
            Log.w(TAG, "mDNS browse not allowed: ${e.message}")
            close()
            return@callbackFlow
        }
        awaitClose { runCatching { nsd.stopServiceDiscovery(listener) } }
    }

    /** Called by the sync core, on its own thread, when a paired Frame moved. */
    override fun find(fingerprint: String, timeoutMs: UInt): List<String> = runBlocking {
        withTimeoutOrNull(timeoutMs.toLong()) { browse().first { it.fingerprint == fingerprint } }?.addrs.orEmpty()
    }

    private fun resolve(info: NsdServiceInfo, found: (FoundFrame) -> Unit) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            val callback = object : NsdManager.ServiceInfoCallback {
                override fun onServiceInfoCallbackRegistrationFailed(errorCode: Int) {}
                override fun onServiceLost() {}
                override fun onServiceInfoCallbackUnregistered() {}

                override fun onServiceUpdated(resolved: NsdServiceInfo) {
                    val frame = frameOf(resolved, resolved.hostAddresses) ?: return
                    // one answer is enough, the next browse asks again
                    runCatching { nsd.unregisterServiceInfoCallback(this) }
                    found(frame)
                }
            }
            runCatching { nsd.registerServiceInfoCallback(info, executor, callback) }
        } else {
            @Suppress("DEPRECATION")
            nsd.resolveService(info, object : NsdManager.ResolveListener {
                override fun onResolveFailed(info: NsdServiceInfo, errorCode: Int) {}

                override fun onServiceResolved(resolved: NsdServiceInfo) {
                    frameOf(resolved, listOfNotNull(resolved.host))?.let(found)
                }
            })
        }
    }

    private fun frameOf(info: NsdServiceInfo, hosts: List<InetAddress>): FoundFrame? {
        val fp = info.attributes["fp"]?.decodeToString()?.lowercase()?.takeIf { it.length == 64 } ?: return null
        val name = info.attributes["name"]?.decodeToString() ?: "Steam Frame"
        // IPv4 first: it's what the Frame advertises and what always routes
        val addrs = hosts.sortedBy { it !is Inet4Address }.map { join(it, info.port) }.distinct()
        if (addrs.isEmpty()) return null
        return FoundFrame(name = name, fingerprint = fp, addr = addrs.first(), addrs = addrs)
    }

    private fun join(ip: InetAddress, port: Int): String {
        val host = ip.hostAddress?.substringBefore('%') ?: ""
        return if (ip is Inet4Address) "$host:$port" else "[$host]:$port"
    }
}
