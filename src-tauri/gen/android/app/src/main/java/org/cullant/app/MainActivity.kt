package org.cullant.app

import android.content.Intent
import android.hardware.usb.UsbManager
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.addCallback
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  private var webView: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
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
    onBackPressedDispatcher.addCallback(this) {
      webView.evaluateJavascript("window.__cullantBack && window.__cullantBack()", null)
    }
  }
}
