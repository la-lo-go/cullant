package org.cullant.app

import android.content.Intent
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.addCallback
import androidx.activity.enableEdgeToEdge
import androidx.core.content.ContextCompat

class MainActivity : TauriActivity() {
  private var webView: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // The window background draws the same mark at the same place as the
    // Android 12 splash, so cut over at once. The system exit animation moves
    // the mark while it fades, which showed as a short jump.
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
      splashScreen.setOnExitAnimationListener { it.remove() }
    }
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    setIntent(intent)
    if (intent.action == UsbManager.ACTION_USB_DEVICE_ATTACHED) {
      webView?.post {
        webView?.evaluateJavascript(
          "window.dispatchEvent(new Event('cullant:usb-attached'))",
          null,
        )
      }
    }
  }

  // Tauri registers its back handler once per process, bound to whichever
  // activity was alive at the time. Android can recreate the activity on its own
  // (memory pressure while backgrounded, "don't keep activities"), and nothing
  // re-registers that handler, so back would finish the app without the page
  // ever hearing about it. Bind one to every activity and send the press to the
  // same place the `back-button` event goes.
  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    // A WebView paints white until the page draws, which flashed between the
    // dark splash and the app.
    webView.setBackgroundColor(ContextCompat.getColor(this, R.color.cullant_bg))
    webView.settings.apply {
      setSupportZoom(false)
      builtInZoomControls = false
      displayZoomControls = false
    }
    onBackPressedDispatcher.addCallback(this) {
      webView.evaluateJavascript("window.__cullantBack && window.__cullantBack()", null)
    }
  }
}
