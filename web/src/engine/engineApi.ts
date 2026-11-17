/** The wasm-bindgen surface of `crates/engine-wasm` the worker uses (generated glue). */
export interface SurfaceApi {
  kind(): string;
  readonly init_ms: number;
  free(): void;
}

export interface EngineApi {
  attach(surface: SurfaceApi): void;
  detach(): void;
  backend(): string;
  start_probe(): void;
  probe_status(): string;
  device_lost(): boolean;
  /** Largest canvas side of the attached backend, device px; 0 when unknown. */
  max_texture_size(): number;
  lose_device(): void;
  /** First uncaptured WebGPU error of the attached surface. */
  gpu_error(): string | undefined;
  present_probe(size: number, rgb: number): void;
  end_present_probe(): void;
  simulate_gpu_fault(fault: string): void;
  set_text_mode(mode: string): void;
  load_snapshot(json: string): void;
  frame_keys(revision: number, frames: string): string;
  required_fonts(
    revision: number,
    frameId: string,
    width: number,
    height: number,
  ): string;
  has_font(id: string, hash: string): boolean;
  load_font(id: string, hash: string, bytes: Uint8Array): void;
  compute(
    revision: number,
    frameId: string,
    width: number,
    height: number,
    digest: string,
  ): string;
  has_image(id: string, hash: string): boolean;
  add_image(
    id: string,
    hash: string,
    rgba: Uint8Array,
    width: number,
    height: number,
  ): void;
  commit(frameId: string): boolean;
  discard(frameId: string): void;
  reuse(frameId: string, digest: string, revision: number): boolean;
  set_layout(json: string): void;
  set_camera(
    scale: number,
    tx: number,
    ty: number,
    width: number,
    height: number,
  ): void;
  render(): string;
  gpu_idle(): boolean;
  hit_test(x: number, y: number, revision: number): string;
  bounds(frameId: string, revision: number, ids: string): string;
  frame_status(): string;
}

export interface EngineModule {
  Engine: new () => EngineApi;
  Surface: {
    webgpu(canvas: OffscreenCanvas): Promise<SurfaceApi>;
    webgl2(canvas: OffscreenCanvas): SurfaceApi;
    cpu(canvas: OffscreenCanvas): SurfaceApi;
  };
}
