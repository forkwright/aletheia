<script lang="ts">
  import { login } from "../../lib/auth";
  import { getBrandName } from "../../stores/branding.svelte";

  let { onSuccess }: { onSuccess: () => void } = $props();

  let username = $state("");
  let password = $state("");
  let rememberMe = $state(true);
  let error = $state<string | null>(null);
  let loading = $state(false);

  async function handleSubmit(e: Event) {
    e.preventDefault();
    if (!username.trim() || !password) return;

    loading = true;
    error = null;

    const result = await login(username.trim(), password, rememberMe);
    loading = false;

    if (result.ok) {
      onSuccess();
    } else {
      error = result.error ?? "Login failed";
    }
  }
</script>

<div class="login-page">
  <div class="login-card">
    <h1>{getBrandName()}</h1>
    <p>Sign in to continue</p>
    <form onsubmit={handleSubmit}>
      <input
        type="text"
        class="input"
        placeholder="Username"
        autocomplete="username"
        bind:value={username}
        disabled={loading}
      />
      <input
        type="password"
        class="input"
        placeholder="Password"
        autocomplete="current-password"
        bind:value={password}
        disabled={loading}
      />
      <label class="remember">
        <input type="checkbox" bind:checked={rememberMe} />
        <span>Remember me</span>
      </label>
      {#if error}
        <div class="error">{error}</div>
      {/if}
      <button type="submit" class="submit" disabled={loading}>
        {loading ? "Signing in..." : "Sign in"}
      </button>
    </form>
  </div>
</div>

<style>
  .login-page {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    background: var(--bg);
  }
  .login-card {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 32px;
    max-width: 380px;
    width: 100%;
    text-align: center;
  }
  .login-card h1 {
    font-family: var(--font-display);
    font-size: var(--text-3xl);
    font-weight: 500;
    letter-spacing: 0.01em;
    margin-bottom: 4px;
  }
  .login-card p {
    color: var(--text-secondary);
    font-size: var(--text-base);
    margin-bottom: 20px;
  }
  .login-card form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .input {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 10px 14px;
    font-size: var(--text-base);
    font-family: var(--font-sans);
    width: 100%;
  }
  .input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .input:disabled {
    opacity: 0.6;
  }
  .remember {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-sm);
    color: var(--text-secondary);
    cursor: pointer;
    user-select: none;
  }
  .remember input[type="checkbox"] {
    accent-color: var(--accent);
  }
  .error {
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid var(--status-error);
    border-radius: var(--radius-sm);
    color: var(--status-error);
    font-size: var(--text-sm);
    padding: 8px 12px;
    text-align: left;
  }
  .submit {
    background: var(--accent);
    border: none;
    color: var(--bg);
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    font-size: var(--text-base);
    font-weight: 500;
    cursor: pointer;
    margin-top: 4px;
  }
  .submit:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .submit:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  @media (max-width: 768px) {
    .login-page {
      height: 100dvh;
      padding: var(--safe-top) 0 var(--safe-bottom);
    }
    .login-card {
      margin: 0 16px;
      padding: 24px 20px;
    }
    .input {
      font-size: var(--text-lg); /* Prevents iOS zoom */
      padding: 12px 14px;
    }
    .submit {
      padding: 12px 14px;
      font-size: var(--text-base);
    }
  }
</style>
