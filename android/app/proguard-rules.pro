# JNA and the UniFFI bindings find their classes and methods by name
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class com.framecorder.core.** { *; }
-dontwarn java.awt.**
