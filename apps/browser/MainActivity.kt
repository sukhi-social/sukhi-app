package dev.dioxus.main

import android.app.NativeActivity
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.view.RoundedCorner
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.os.Handler
import android.os.Looper
import java.io.File
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup

class MainActivity : NativeActivity() {

    // one should probably traverse the view tree to find the rendering surface. this will work regardless of manufacturer.
    // example: if oem wraps those three previous getChildAt will not work. 
    // the only thing that matters here is surfaceview. 
    private fun findNativeSurfaceView(view: View): View? {
        if (view is SurfaceView) return view
        
        if (view is ViewGroup) {
            for (i in 0 until view.childCount) {
                val found = findNativeSurfaceView(view.getChildAt(i))
                if (found != null) return found
            }
        }
        return null
    }

    // sukhi shell: a returning URL (login callback) is handed to Rust through a file only this app can write.
    private fun stashCallback(intent: Intent?) {
        val data = intent?.dataString ?: return
        if (intent.action == Intent.ACTION_VIEW && data.startsWith("social.sukhi.shell://")) {
            File(filesDir, "shell-callback.txt").writeText(data)
        }
    }

    // sukhi shell: Rust asks to leave to the background (back pressed at the bottom of the stack)
    // by writing a file; no JNI.
    private val exitWatcher = Handler(Looper.getMainLooper())
    private val exitCheck = object : Runnable {
        override fun run() {
            val f = File(filesDir, "shell-exit.txt")
            if (f.exists()) {
                f.delete()
                moveTaskToBack(true)
            }
            exitWatcher.postDelayed(this, 250)
        }
    }

    // sukhi shell: the system bars, cutout, keyboard and screen corners, in dp, for Rust to read.
    // top,right,bottom,left, keyboard-bottom, corner radii TL,TR,BR,BL
    private fun publishInsets(insets: WindowInsets) {
        val d = resources.displayMetrics.density
        val top: Int; val right: Int; val bottom: Int; val left: Int; val ime: Int
        if (Build.VERSION.SDK_INT >= 30) {
            val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            top = bars.top; right = bars.right; bottom = bars.bottom; left = bars.left
            ime = insets.getInsets(WindowInsets.Type.ime()).bottom
        } else {
            @Suppress("DEPRECATION")
            run {
                top = insets.systemWindowInsetTop; right = insets.systemWindowInsetRight
                bottom = insets.systemWindowInsetBottom; left = insets.systemWindowInsetLeft
                ime = 0
            }
        }
        fun corner(pos: Int): Int =
            if (Build.VERSION.SDK_INT >= 31) (insets.getRoundedCorner(pos)?.radius ?: 0) else 0
        val px = listOf(
            top, right, bottom, left, ime,
            corner(RoundedCorner.POSITION_TOP_LEFT), corner(RoundedCorner.POSITION_TOP_RIGHT),
            corner(RoundedCorner.POSITION_BOTTOM_RIGHT), corner(RoundedCorner.POSITION_BOTTOM_LEFT),
        )
        val line = px.joinToString(",") { "%.1f".format(it / d) }
        val f = File(filesDir, "shell-insets.txt")
        if (!f.exists() || f.readText() != line) f.writeText(line)
    }

    override fun onDestroy() {
        exitWatcher.removeCallbacks(exitCheck)
        super.onDestroy()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        stashCallback(intent)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        stashCallback(intent)
        exitWatcher.post(exitCheck)
        // The page ground is light (cream), so the system bar icons turn dark.
        if (Build.VERSION.SDK_INT >= 30) {
            val light = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or
                WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
            window.insetsController?.setSystemBarsAppearance(light, light)
        }
        window.decorView.setOnApplyWindowInsetsListener { v, insets ->
            publishInsets(insets)
            v.onApplyWindowInsets(insets)
        }

        // wait until android has actually finished building the UI layout before search.
        window.decorView.post {
            val nativeView = findNativeSurfaceView(window.decorView)
            
            
            nativeView?.apply {
                isFocusable = true
                isFocusableInTouchMode = true
                requestFocus()
            }
        }
    }
}
