#include <math.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>

#include "mlx/c/mlx.h"

static char last_error[4096];
static bool saw_no_metal_device;

static void record_error(const char* msg, void* data) {
  (void)data;
  snprintf(last_error, sizeof(last_error), "%s", msg ? msg : "");
  if (msg && strstr(msg, "No Metal device available") != NULL) {
    saw_no_metal_device = true;
  }
}

static int check(int code, const char* label) {
  if (code != 0) {
    fprintf(
        stderr,
        "%s failed with code %d%s%s\n",
        label,
        code,
        last_error[0] ? ": " : "",
        last_error);
    return 1;
  }
  return 0;
}

static bool metal_device_unavailable(void) {
  return saw_no_metal_device;
}

static int finish_failed(void) {
  if (metal_device_unavailable()) {
    printf("mlx_smoke=unavailable reason=no-metal-device\n");
    return 77;
  }
  return 1;
}

int main(void) {
  mlx_set_error_handler(record_error, NULL, NULL);

  mlx_string version = mlx_string_new();
  if (check(mlx_version(&version), "mlx_version")) {
    return 1;
  }
  printf("mlx_version=%s\n", mlx_string_data(version));

  bool metal = false;
  if (check(mlx_metal_is_available(&metal), "mlx_metal_is_available")) {
    mlx_string_free(version);
    return 1;
  }
  printf("mlx_metal=%s\n", metal ? "true" : "false");

  mlx_device cpu = mlx_device_new_type(MLX_CPU, 0);
  if (check(mlx_set_default_device(cpu), "mlx_set_default_device")) {
    mlx_device_free(cpu);
    mlx_string_free(version);
    return 1;
  }

  mlx_stream stream = mlx_default_cpu_stream_new();
  float values[] = {1.0f, 2.0f, 3.0f, 4.0f};
  int shape[] = {2, 2};
  mlx_array input = mlx_array_new();
  mlx_array two = mlx_array_new();
  mlx_array added = mlx_array_new();
  mlx_array total = mlx_array_new();

  int failed = 0;
  failed |= check(mlx_array_set_data(&input, values, shape, 2, MLX_FLOAT32), "mlx_array_set_data");
  if (failed) {
    mlx_array_free(total);
    mlx_array_free(added);
    mlx_array_free(two);
    mlx_array_free(input);
    mlx_stream_free(stream);
    mlx_device_free(cpu);
    mlx_string_free(version);
    return finish_failed();
  }
  failed |= check(mlx_array_set_float32(&two, 2.0f), "mlx_array_set_float32");
  if (failed) {
    mlx_array_free(total);
    mlx_array_free(added);
    mlx_array_free(two);
    mlx_array_free(input);
    mlx_stream_free(stream);
    mlx_device_free(cpu);
    mlx_string_free(version);
    return finish_failed();
  }
  failed |= check(mlx_add(&added, input, two, stream), "mlx_add");
  failed |= check(mlx_sum(&total, added, false, stream), "mlx_sum");
  failed |= check(mlx_synchronize(stream), "mlx_synchronize");

  float result = 0.0f;
  failed |= check(mlx_array_item_float32(&result, total), "mlx_array_item_float32");
  printf("mlx_smoke_sum=%.1f\n", result);

  if (!failed && fabsf(result - 18.0f) > 0.001f) {
    fprintf(stderr, "unexpected MLX smoke sum: %.6f\n", result);
    failed = 1;
  }

  mlx_array_free(total);
  mlx_array_free(added);
  mlx_array_free(two);
  mlx_array_free(input);
  mlx_stream_free(stream);
  mlx_device_free(cpu);
  mlx_string_free(version);

  if (failed) {
    return finish_failed();
  }
  printf("mlx_smoke=ok\n");
  return 0;
}
