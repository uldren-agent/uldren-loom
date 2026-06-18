document.querySelectorAll(".chart-figure").forEach((figure) => {
  const canvas = figure.querySelector("canvas");
  const source = figure.querySelector('script[type="application/json"]');

  if (!canvas || !source || !window.Chart) {
    return;
  }

  try {
    const config = JSON.parse(source.textContent);
    new window.Chart(canvas, config);
  } catch (error) {
    figure.dataset.chartError = error.message;
  }
});
