package io.github.nonock.eztravel

import android.Manifest
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.automirrored.outlined.List
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Warning
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.lifecycle.lifecycleScope
import com.google.firebase.messaging.FirebaseMessaging
import io.github.nonock.eztravel.backend.AppBackend
import io.github.nonock.eztravel.home.AlertsPage
import io.github.nonock.eztravel.home.CommuteStatus
import io.github.nonock.eztravel.home.NextDepartures
import io.github.nonock.eztravel.home.NotificationHistory
import io.github.nonock.eztravel.notifications.Notifier
import io.github.nonock.eztravel.settings.AwaySetting
import io.github.nonock.eztravel.ui.theme.EztravelTheme
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val askToNotify = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        // The logo on navy while the app starts, then the app's own theme.
        installSplashScreen()
        super.onCreate(savedInstanceState)
        Notifier.createChannel(this)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            askToNotify.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        // The backend learns where to send notifications; again at each launch, in case it failed.
        FirebaseMessaging.getInstance().token.addOnSuccessListener { token ->
            lifecycleScope.launch(Dispatchers.IO) { AppBackend.register(token) }
        }
        // Light icons in the system bars: the app is always navy.
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
        )
        setContent {
            EztravelTheme { App() }
        }
    }
}

/** The pages of the bottom bar: [title] heads the page, [label] names its tab. */
enum class Tab(@StringRes val label: Int, @StringRes val title: Int, val icon: ImageVector, val selected: ImageVector) {
    TRIP(R.string.nav_trip, R.string.app_name, Icons.Outlined.Home, Icons.Filled.Home),
    ALERTS(R.string.nav_alerts, R.string.nav_alerts, Icons.Outlined.Warning, Icons.Filled.Warning),
    HISTORY(R.string.nav_history, R.string.history_title, Icons.AutoMirrored.Outlined.List, Icons.AutoMirrored.Filled.List),
}

/**
 * The commute, the lines' alerts and the history of notifications, one tab
 * each, read again when pulled down; the settings behind the gear. Back
 * leaves the settings, then goes back to the commute.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun App() {
    var tab by rememberSaveable { mutableStateOf(Tab.TRIP) }
    var settings by rememberSaveable { mutableStateOf(false) }
    // Read again when pulled down, and after the time away changes: it moves the next watch.
    var refresh by remember { mutableIntStateOf(0) }
    var refreshing by remember { mutableStateOf(false) }
    val read = { refreshing = false }
    BackHandler(enabled = settings || tab != Tab.TRIP) {
        if (settings) settings = false else tab = Tab.TRIP
    }
    Scaffold(
        modifier = Modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = { Text(stringResource(if (settings) R.string.settings_title else tab.title)) },
                navigationIcon = {
                    if (settings) {
                        IconButton(onClick = { settings = false }) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                        }
                    }
                },
                actions = {
                    if (!settings) {
                        IconButton(onClick = { settings = true }) {
                            Icon(Icons.Outlined.Settings, contentDescription = stringResource(R.string.settings_title))
                        }
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        bottomBar = {
            if (!settings) {
                NavigationBar {
                    for (each in Tab.entries) {
                        NavigationBarItem(
                            selected = tab == each,
                            onClick = { tab = each },
                            icon = { Icon(if (tab == each) each.selected else each.icon, contentDescription = null) },
                            label = { Text(stringResource(each.label)) },
                        )
                    }
                }
            }
        },
    ) { padding ->
        val backend = AppBackend.current
        val page = @Composable {
            Column(
                Modifier
                    .fillMaxSize()
                    .padding(horizontal = 16.dp)
                    .padding(bottom = 16.dp)
                    .verticalScroll(rememberScrollState()),
            ) {
                when {
                    settings -> AwaySetting(backend, onSaved = { refresh++ })
                    tab == Tab.TRIP -> {
                        CommuteStatus(backend, refresh = refresh)
                        NextDepartures(backend, Modifier.padding(top = 16.dp), refresh = refresh, onRead = read)
                    }
                    tab == Tab.ALERTS -> AlertsPage(backend, refresh = refresh, onRead = read)
                    else -> NotificationHistory(backend, refresh = refresh, onRead = read)
                }
            }
        }
        // Without a server, nothing would be read again.
        if (settings || !backend.configured) {
            Box(Modifier.padding(padding)) { page() }
        } else {
            PullToRefreshBox(
                isRefreshing = refreshing,
                onRefresh = {
                    refreshing = true
                    refresh++
                },
                modifier = Modifier.padding(padding),
            ) { page() }
        }
    }
}
