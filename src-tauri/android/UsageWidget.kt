package __PACKAGE__

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.view.View
import android.widget.RemoteViews
import java.io.File
import org.json.JSONObject

/**
 * The home-screen widget: the three fullest limits across every account.
 *
 * It draws what the app last saved (state.json and snapshots.json in the app's data folder), so it
 * is as fresh as the last time the app refreshed. It never touches the network or a credential.
 */
class UsageWidget : AppWidgetProvider() {
  override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
    val views = render(context)
    for (id in ids) manager.updateAppWidget(id, views)
  }

  companion object {
    private val ROWS = intArrayOf(R.id.widget_row1, R.id.widget_row2, R.id.widget_row3)
    private val NAMES = intArrayOf(R.id.widget_name1, R.id.widget_name2, R.id.widget_name3)
    private val VALUES = intArrayOf(R.id.widget_value1, R.id.widget_value2, R.id.widget_value3)
    private val BARS = intArrayOf(R.id.widget_bar1, R.id.widget_bar2, R.id.widget_bar3)
    private val PROVIDERS = mapOf(
      "claude" to "Claude", "codex" to "ChatGPT", "copilot" to "Copilot", "cursor" to "Cursor",
      "kimi" to "Kimi", "openrouter" to "OpenRouter", "zai" to "Z.ai", "ollama" to "Ollama",
      "grok" to "Grok", "amp" to "Amp", "opencode" to "OpenCode", "muse" to "Muse Code",
      "devin" to "Devin", "copilotorg" to "Copilot Org", "claudeapi" to "Claude API",
    )

    private class Limit(val name: String, val percent: Int)

    /** Redraws every placed widget. The app calls this when it goes to the background. */
    fun refresh(context: Context) {
      val manager = AppWidgetManager.getInstance(context)
      val ids = manager.getAppWidgetIds(ComponentName(context, UsageWidget::class.java))
      if (ids.isEmpty()) return
      val views = render(context)
      for (id in ids) manager.updateAppWidget(id, views)
    }

    private fun readJson(context: Context, name: String): JSONObject? =
      try {
        JSONObject(File(context.applicationInfo.dataDir, name).readText())
      } catch (error: Exception) {
        null
      }

    /** Every limit with a bar, fullest first. Empty when the app has saved nothing yet. */
    private fun limits(context: Context): List<Limit> {
      val accounts = readJson(context, "state.json")?.optJSONArray("accounts") ?: return emptyList()
      val snapshots = readJson(context, "snapshots.json") ?: return emptyList()
      val found = ArrayList<Limit>()
      for (index in 0 until accounts.length()) {
        val account = accounts.optJSONObject(index) ?: continue
        val provider = account.optString("provider")
        val lines = snapshots.optJSONObject(account.optString("id"))?.optJSONArray("lines") ?: continue
        for (lineIndex in 0 until lines.length()) {
          val line = lines.optJSONObject(lineIndex) ?: continue
          val limit = line.optDouble("limit", 0.0)
          if (line.optString("kind") != "progress" || limit <= 0.0) continue
          val percent = (line.optDouble("used", 0.0) / limit * 100.0).coerceIn(0.0, 100.0)
          found.add(Limit("${PROVIDERS[provider] ?: provider} · ${line.optString("label")}", Math.round(percent).toInt()))
        }
      }
      return found.sortedByDescending { it.percent }
    }

    private fun render(context: Context): RemoteViews {
      val views = RemoteViews(context.packageName, R.layout.usage_widget)
      val limits = limits(context)
      views.setViewVisibility(R.id.widget_empty, if (limits.isEmpty()) View.VISIBLE else View.GONE)
      for (row in ROWS.indices) {
        val limit = limits.getOrNull(row)
        views.setViewVisibility(ROWS[row], if (limit == null) View.GONE else View.VISIBLE)
        if (limit == null) continue
        views.setTextViewText(NAMES[row], limit.name)
        views.setTextViewText(VALUES[row], "${limit.percent}%")
        views.setProgressBar(BARS[row], 100, limit.percent, false)
      }
      val open = Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      val flags = PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
      views.setOnClickPendingIntent(R.id.widget_root, PendingIntent.getActivity(context, 0, open, flags))
      return views
    }
  }
}
