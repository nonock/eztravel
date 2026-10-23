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
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.lifecycle.lifecycleScope
import io.github.nonock.eztravel.backend.AppBackend
import io.github.nonock.eztravel.calendar.CalendarPage
import io.github.nonock.eztravel.home.AlertsPage
import io.github.nonock.eztravel.home.CommuteStatus
import io.github.nonock.eztravel.home.NextDepartures
import io.github.nonock.eztravel.home.NotificationHistory
import io.github.nonock.eztravel.notifications.Notifier
import io.github.nonock.eztravel.notifications.PushToken
import io.github.nonock.eztravel.settings.SettingsMenu
import io.github.nonock.eztravel.settings.SettingsPage
import io.github.nonock.eztravel.settings.UsualTrainSetting
import io.github.nonock.eztravel.watchdog.Watchdog
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
        PushToken.fetch { token ->
            lifecycleScope.launch(Dispatchers.IO) { AppBackend.register(token) }
        }
        // The phone looks at the backend by itself during the windows: from now, the next look is set.
        lifecycleScope.launch(Dispatchers.IO) { Watchdog.run(applicationContext, notify = false) }
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
}

/**
 * The commute and the lines' alerts, one tab each; behind the gear, a list
 * of the calendar, the usual trains and the history of notifications, each
 * its own page. Pages are read again when pulled down, but for the list and
 * the usual trains. Back leaves a page for the list, the list for the tabs,
 * then goes back to the commute.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun App() {
    var tab by rememberSaveable { mutableStateOf(Tab.TRIP) }
    // Behind the gear: the list, then the page opened from it.
    var settings by rememberSaveable { mutableStateOf(false) }
    var opened by rememberSaveable { mutableStateOf<SettingsPage?>(null) }
    // Read again when pulled down, and after the days change: they move the next watch.
    var refresh by remember { mutableIntStateOf(0) }
    var refreshing by remember { mutableStateOf(false) }
    val read = { refreshing = false }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    // A day changed may move the next watch: the status is read again, and the watchdog's next look set again.
    val daysSaved: () -> Unit = {
        refresh++
        scope.launch(Dispatchers.IO) { Watchdog.run(context.applicationContext, notify = false) }
    }
    val leave = {
        if (opened != null) opened = null else settings = false
    }
    BackHandler(enabled = settings || tab != Tab.TRIP) {
        if (settings) leave() else tab = Tab.TRIP
    }
    Scaffold(
        modifier = Modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = {
                    val title = when {
                        opened != null -> opened!!.title
                        settings -> R.string.settings_title
                        else -> tab.title
                    }
                    Text(stringResource(title))
                },
                navigationIcon = {
                    if (settings) {
                        IconButton(onClick = leave) {
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
                    opened == SettingsPage.CALENDAR -> CalendarPage(backend, refresh = refresh, onRead = read, onSaved = daysSaved)
                    opened == SettingsPage.USUAL_TRAINS -> UsualTrainSetting(backend, onSaved = { refresh++ })
                    opened == SettingsPage.HISTORY -> NotificationHistory(backend, refresh = refresh, onRead = read)
                    settings -> {
                        if (!backend.configured) Text(stringResource(R.string.not_configured))
                        SettingsMenu(onOpen = { opened = it })
                    }
                    tab == Tab.TRIP -> {
                        CommuteStatus(backend, refresh = refresh)
                        NextDepartures(backend, Modifier.padding(top = 16.dp), refresh = refresh, onRead = read)
                    }
                    else -> AlertsPage(backend, refresh = refresh, onRead = read)
                }
            }
        }
        // Without a server, nothing would be read again; nor is the list, nor the usual trains.
        val readAgain = !settings || opened == SettingsPage.CALENDAR || opened == SettingsPage.HISTORY
        if (!readAgain || !backend.configured) {
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
