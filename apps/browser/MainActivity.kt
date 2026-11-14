package dev.dioxus.main

import android.app.NativeActivity
import android.content.Intent
import android.os.Bundle
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
