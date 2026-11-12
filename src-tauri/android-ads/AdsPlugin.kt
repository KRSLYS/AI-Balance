package __PACKAGE__

import android.app.Activity
import android.view.Gravity
import android.view.ViewGroup
import android.widget.FrameLayout
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.android.gms.ads.AdRequest
import com.google.android.gms.ads.AdSize
import com.google.android.gms.ads.AdView
import com.google.android.gms.ads.MobileAds
import com.google.android.ump.ConsentInformation
import com.google.android.ump.ConsentRequestParameters
import com.google.android.ump.UserMessagingPlatform

@InvokeArg
class BannerArgs {
  lateinit var unitId: String
}

/**
 * The store edition's one ad: a banner at the bottom of the screen.
 *
 * Nothing is requested from the ad network until Google's consent flow says ads may be requested.
 * Where the law requires consent, that means after the user has answered the consent form; a user
 * who declines still gets the whole app. This class is only compiled into the store edition.
 */
@TauriPlugin
class AdsPlugin(private val activity: Activity) : Plugin(activity) {
  private var banner: AdView? = null
  private var bannerHeightDp = 0

  /** Runs the consent flow, then shows the banner. Resolves with the banner's height in dp, 0 for none. */
  @Command
  fun showBanner(invoke: Invoke) {
    val unitId = invoke.parseArgs(BannerArgs::class.java).unitId
    activity.runOnUiThread {
      try {
        val consent = UserMessagingPlatform.getConsentInformation(activity)
        consent.requestConsentInfoUpdate(
          activity,
          ConsentRequestParameters.Builder().build(),
          { UserMessagingPlatform.loadAndShowConsentFormIfRequired(activity) { resolveBanner(invoke, consent, unitId) } },
          { resolveBanner(invoke, consent, unitId) },
        )
      } catch (error: Exception) {
        invoke.resolve(JSObject().put("height", 0))
      }
    }
  }

  private fun resolveBanner(invoke: Invoke, consent: ConsentInformation, unitId: String) {
    try {
      if (consent.canRequestAds() && banner == null) addBanner(unitId)
    } catch (error: Exception) {
      // An ad that cannot load must never get in the way of the app.
    }
    invoke.resolve(JSObject().put("height", bannerHeightDp))
  }

  private fun addBanner(unitId: String) {
    MobileAds.initialize(activity) {}
    val metrics = activity.resources.displayMetrics
    val size = AdSize.getCurrentOrientationAnchoredAdaptiveBannerAdSize(activity, (metrics.widthPixels / metrics.density).toInt())
    val view = AdView(activity)
    view.setAdSize(size)
    view.adUnitId = unitId
    val params = FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.BOTTOM)
    // The app draws edge to edge, so keep the banner above the system navigation bar.
    ViewCompat.setOnApplyWindowInsetsListener(view) { banner, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
      (banner.layoutParams as FrameLayout.LayoutParams).bottomMargin = bars.bottom
      banner.requestLayout()
      insets
    }
    activity.addContentView(view, params)
    view.loadAd(AdRequest.Builder().build())
    banner = view
    bannerHeightDp = size.height
  }

  /** Whether the user is entitled to change their answer, which is the case wherever consent was asked. */
  @Command
  fun privacyOptionsRequired(invoke: Invoke) {
    val status = UserMessagingPlatform.getConsentInformation(activity).privacyOptionsRequirementStatus
    invoke.resolve(JSObject().put("required", status == ConsentInformation.PrivacyOptionsRequirementStatus.REQUIRED))
  }

  /** Opens Google's form for changing the consent answer. */
  @Command
  fun showPrivacyOptions(invoke: Invoke) {
    activity.runOnUiThread {
      UserMessagingPlatform.showPrivacyOptionsForm(activity) { invoke.resolve() }
    }
  }
}
