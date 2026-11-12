package __PACKAGE__

import android.app.Activity
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
class VaultArgs {
  lateinit var data: String
}

/**
 * Encrypts the saved sign-ins with a key that lives in the Android Keystore.
 *
 * The key is generated inside the Keystore and cannot be exported, so the encrypted file is useless
 * on any other device, in a backup, or to another app that manages to copy it. The app's Rust core
 * calls `seal` before it writes the file and `unseal` after it reads it.
 */
@TauriPlugin
class VaultPlugin(activity: Activity) : Plugin(activity) {
  private fun key(): SecretKey {
    val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }
    (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
    val spec = KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
      .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
      .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
      .setKeySize(256)
      .build()
    return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE).apply { init(spec) }.generateKey()
  }

  /** Returns base64 of a fresh nonce followed by the AES-GCM ciphertext. */
  @Command
  fun seal(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(VaultArgs::class.java)
      val cipher = Cipher.getInstance(TRANSFORMATION).apply { init(Cipher.ENCRYPT_MODE, key()) }
      val sealed = cipher.iv + cipher.doFinal(args.data.toByteArray(Charsets.UTF_8))
      invoke.resolve(JSObject().put("data", Base64.encodeToString(sealed, Base64.NO_WRAP)))
    } catch (error: Exception) {
      invoke.reject("Could not encrypt: ${error.javaClass.simpleName}")
    }
  }

  @Command
  fun unseal(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(VaultArgs::class.java)
      val sealed = Base64.decode(args.data, Base64.NO_WRAP)
      val cipher = Cipher.getInstance(TRANSFORMATION)
      cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, sealed, 0, NONCE_BYTES))
      val plain = cipher.doFinal(sealed, NONCE_BYTES, sealed.size - NONCE_BYTES)
      invoke.resolve(JSObject().put("data", String(plain, Charsets.UTF_8)))
    } catch (error: Exception) {
      invoke.reject("Could not decrypt: ${error.javaClass.simpleName}")
    }
  }

  companion object {
    private const val KEYSTORE = "AndroidKeyStore"
    private const val ALIAS = "aibalance.credentials"
    private const val TRANSFORMATION = "AES/GCM/NoPadding"
    private const val NONCE_BYTES = 12
    private const val TAG_BITS = 128
  }
}
