/**
 * The part of three the Knot uses. Importing the whole namespace dynamically keeps every export
 * alive; naming them here lets the bundler drop the rest of the library from the Knot chunk.
 */
export {
  BufferAttribute, BufferGeometry, Color, DoubleSide, DynamicDrawUsage, Float32BufferAttribute, Fog, Mesh,
  MeshBasicMaterial, PerspectiveCamera, Raycaster, Scene, SRGBColorSpace, Texture, Vector2, WebGLRenderer,
} from 'three';
