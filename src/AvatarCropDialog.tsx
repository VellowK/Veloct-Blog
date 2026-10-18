/**
 * The avatar crop dialog.
 *
 * `handoff.md` §3 keeps Markdown and the site's own files as the truth, so an avatar is a FILE the
 * site owns rather than a value the database remembers. That decides the shape of this component:
 * it produces the final PNG bytes here, in the window, and the only thing that crosses the IPC
 * boundary is the picture that will actually be stored.
 *
 * ## Why the chosen file is read through `FileReader` and not `URL.createObjectURL`
 *
 * The shell's CSP (`src-tauri/tauri.conf.json`) allows `img-src 'self' asset: http://asset.localhost
 * data: https:` — and no `blob:`. An object URL would therefore be refused by the WebView, and the
 * failure looks like "the picture never loads" rather than like a policy problem. A data URL is
 * allowed, so that is what the source image is decoded from.
 *
 * ## Why the geometry is computed rather than dragged as a DOM transform
 *
 * The exported pixels have to be the pixels the user framed. Deriving one source rectangle from the
 * same `zoom`/`offset` values that position the preview is what makes the two agree; measuring the
 * element after a CSS transform would introduce a second, independently rounded answer.
 */

import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';

/** The crop frame is square, so it can be measured once and used directly in the arithmetic. */
const FRAME = 288;

/** What is actually stored. Large enough for a retina avatar, small enough to stay a small file. */
const OUTPUT_SIZE = 512;

const MIN_ZOOM = 1;
const MAX_ZOOM = 4;

interface AvatarCropDialogProps {
  file: File;
  onCancel: () => void;
  /** Receives the cropped PNG as bare base64, with no `data:` prefix. */
  onConfirm: (dataBase64: string) => void;
  /** True while the caller is storing the result, so the dialog cannot be double-confirmed. */
  busy?: boolean;
}

interface Point {
  x: number;
  y: number;
}

export default function AvatarCropDialog({ file, onCancel, onConfirm, busy = false }: AvatarCropDialogProps) {
  const [image, setImage] = useState<HTMLImageElement | null>(null);
  const [loadError, setLoadError] = useState('');
  const [zoom, setZoom] = useState(MIN_ZOOM);
  const [offset, setOffset] = useState<Point>({ x: 0, y: 0 });
  const dragRef = useRef<{ pointerId: number; start: Point; origin: Point } | null>(null);

  // Decode the chosen file once. `cancelled` matters because reading is asynchronous and the user
  // can close the dialog (or choose another file) before it finishes.
  useEffect(() => {
    let cancelled = false;
    setImage(null);
    setLoadError('');
    setZoom(MIN_ZOOM);
    setOffset({ x: 0, y: 0 });
    const reader = new FileReader();
    reader.onerror = () => { if (!cancelled) setLoadError('无法读取这个文件。'); };
    reader.onload = () => {
      const source = String(reader.result ?? '');
      const decoded = new Image();
      decoded.onload = () => {
        if (cancelled) return;
        if (decoded.naturalWidth === 0 || decoded.naturalHeight === 0) {
          setLoadError('这张图片没有可用的像素，可能已损坏。');
          return;
        }
        setImage(decoded);
      };
      decoded.onerror = () => { if (!cancelled) setLoadError('这个文件不是浏览器能解码的图片。'); };
      decoded.src = source;
    };
    reader.readAsDataURL(file);
    return () => { cancelled = true; };
  }, [file]);

  /** How much the image is magnified over "exactly covers the frame". */
  const coverScale = image ? FRAME / Math.min(image.naturalWidth, image.naturalHeight) : 0;
  const scale = coverScale * zoom;
  const displayWidth = image ? image.naturalWidth * scale : 0;
  const displayHeight = image ? image.naturalHeight * scale : 0;

  /**
   * Keep the frame fully covered.
   *
   * Without this the picture could be dragged off the frame and the export would contain empty
   * pixels, which is the one outcome a cropper must not produce.
   */
  const clamp = useCallback((next: Point, width: number, height: number): Point => ({
    x: Math.min(Math.max(next.x, -(width - FRAME) / 2), (width - FRAME) / 2),
    y: Math.min(Math.max(next.y, -(height - FRAME) / 2), (height - FRAME) / 2),
  }), []);

  // Zooming out shrinks the slack, so an offset that was legal at 3× must be pulled back in.
  useEffect(() => {
    if (!image) return;
    setOffset((current) => clamp(current, displayWidth, displayHeight));
  }, [image, displayWidth, displayHeight, clamp]);

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!image || busy) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      pointerId: event.pointerId,
      start: { x: event.clientX, y: event.clientY },
      origin: offset,
    };
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    setOffset(clamp(
      { x: drag.origin.x + (event.clientX - drag.start.x), y: drag.origin.y + (event.clientY - drag.start.y) },
      displayWidth,
      displayHeight,
    ));
  };

  const endDrag = () => { dragRef.current = null; };

  /** Draw exactly the framed square into the output canvas and hand back its PNG bytes. */
  const confirm = () => {
    if (!image || busy) return;
    const canvas = document.createElement('canvas');
    canvas.width = OUTPUT_SIZE;
    canvas.height = OUTPUT_SIZE;
    const context = canvas.getContext('2d');
    if (!context) {
      setLoadError('这个环境无法创建画布，因此不能裁切图片。');
      return;
    }
    // The frame's top-left corner, in image pixels, is the negative of where the image's top-left
    // corner sits relative to the frame.
    const left = (FRAME - displayWidth) / 2 + offset.x;
    const top = (FRAME - displayHeight) / 2 + offset.y;
    const sourceSize = FRAME / scale;
    context.drawImage(image, -left / scale, -top / scale, sourceSize, sourceSize, 0, 0, OUTPUT_SIZE, OUTPUT_SIZE);
    const dataUrl = canvas.toDataURL('image/png');
    const marker = 'base64,';
    const index = dataUrl.indexOf(marker);
    if (index < 0) {
      setLoadError('画布没有生成 PNG 数据。');
      return;
    }
    onConfirm(dataUrl.slice(index + marker.length));
  };

  const source = `${file.name}${image ? ` · ${image.naturalWidth}×${image.naturalHeight}` : ''}`;

  return (
    <div className="modal-backdrop" role="presentation">
      <div className="modal modal--crop" role="dialog" aria-modal="true" aria-label="裁切首页头像">
        <div className="modal-header">
          <strong>裁切首页头像</strong>
          <button type="button" onClick={onCancel} disabled={busy}>取消</button>
        </div>
        <div className="modal-body">
          {loadError ? (
            <p className="settings-error">{loadError}</p>
          ) : (
            <>
              <p className="muted">{source}</p>
              <div className="crop-stage">
                <div
                  className="crop-frame"
                  style={{ width: FRAME, height: FRAME }}
                  onPointerDown={onPointerDown}
                  onPointerMove={onPointerMove}
                  onPointerUp={endDrag}
                  onPointerCancel={endDrag}
                >
                  {image && (
                    <img
                      src={image.src}
                      alt=""
                      draggable={false}
                      style={{
                        width: displayWidth,
                        height: displayHeight,
                        // Centred on the frame, then moved by the drag offset: the same two values
                        // `confirm` turns into a source rectangle.
                        transform: `translate(-50%, -50%) translate(${offset.x}px, ${offset.y}px)`,
                      }}
                    />
                  )}
                </div>
              </div>
              <label className="crop-zoom">
                <span>缩放</span>
                <input
                  type="range"
                  min={MIN_ZOOM}
                  max={MAX_ZOOM}
                  step={0.01}
                  value={zoom}
                  disabled={!image || busy}
                  onChange={(event) => setZoom(Number(event.target.value))}
                />
              </label>
              <p className="muted">拖动图片调整位置，用滑块缩放。确认后将裁成正方形加入附件池，并上传至首选附件目标。</p>
            </>
          )}
        </div>
        <div className="modal-footer">
          <button type="button" onClick={onCancel} disabled={busy}>取消</button>
          <button type="button" className="primary" onClick={confirm} disabled={!image || busy || !!loadError}>
            {busy ? '正在保存…' : '使用这张头像'}
          </button>
        </div>
      </div>
    </div>
  );
}
