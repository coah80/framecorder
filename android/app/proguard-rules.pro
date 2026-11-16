# JNA and the UniFFI bindings find their classes and methods by name
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class com.framecorder.core.** { *; }
-dontwarn java.awt.**
# ML Kit's qr scanner builds its parts by reflection, and shrinking them away
# crashes "scan the qr code" in release builds only
-keep class com.google.mlkit.** { *; }
-keep class com.google.android.gms.internal.mlkit_code_scanner.** { *; }
