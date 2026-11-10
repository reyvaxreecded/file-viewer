const theme = localStorage.getItem('glassftp.theme');
if (theme === 'light') document.documentElement.dataset.theme = theme;
try {
  const { default: init } = await import('/pkg/glassftp_web.js');
  await init();
} catch (error) {
  const message = document.createElement('div');
  message.className = 'boot-error';
  message.textContent = 'Impossible de démarrer GlassFTP. Vérifiez les assets WebAssembly et rechargez la page.';
  document.body.append(message);
  console.error(error);
}
