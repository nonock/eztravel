package io.github.nonock.eztravel

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Scaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.lifecycleScope
import com.google.firebase.messaging.FirebaseMessaging
import io.github.nonock.eztravel.backend.AppBackend
import io.github.nonock.eztravel.home.CommuteStatus
import io.github.nonock.eztravel.notifications.Notifier
import io.github.nonock.eztravel.settings.AwaySetting
import io.github.nonock.eztravel.ui.theme.EztravelTheme
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val askToNotify = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
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
        enableEdgeToEdge()
        setContent {
            EztravelTheme {
                Scaffold(modifier = Modifier.fillMaxSize()) { innerPadding ->
                    Home(Modifier.padding(innerPadding))
                }
            }
        }
    }
}

@Composable
fun Home(modifier: Modifier = Modifier) {
    // Time away moves the next watch: the status is read again after each change.
    var saves by remember { mutableIntStateOf(0) }
    Column(modifier.padding(16.dp).verticalScroll(rememberScrollState())) {
        CommuteStatus(AppBackend.current, refresh = saves)
        Spacer(Modifier.height(24.dp))
        AwaySetting(AppBackend.current, onSaved = { saves++ })
    }
}
