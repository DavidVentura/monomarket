import { useEffect, useRef } from "react";
import * as THREE from "three";

interface SpinningCoinProps {
  size?: number;
}

export function SpinningCoin({ size = 120 }: SpinningCoinProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const sceneRef = useRef<{
    scene: THREE.Scene;
    camera: THREE.PerspectiveCamera;
    renderer: THREE.WebGLRenderer;
    coinGroup: THREE.Group;
    cleanup: () => void;
  } | null>(null);

  useEffect(() => {
    if (!containerRef.current) return;

    const container = containerRef.current;
    const scene = new THREE.Scene();
    scene.background = null;

    const camera = new THREE.PerspectiveCamera(45, 1, 0.1, 1000);
    camera.position.set(0, 8, 0);
    camera.lookAt(0, 0, 0);

    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    renderer.setSize(size, size);
    container.appendChild(renderer.domElement);

    const ambientLight = new THREE.AmbientLight(0xffffff, 0.6);
    scene.add(ambientLight);

    const directionalLight = new THREE.DirectionalLight(0xffffff, 0.8);
    directionalLight.position.set(0, 10, 7);
    scene.add(directionalLight);

    const pointLight = new THREE.PointLight(0xffa500, 0.4);
    pointLight.position.set(-5, 5, 5);
    scene.add(pointLight);

    const coinGroup = new THREE.Group();

    const coinGeometry = new THREE.CylinderGeometry(2, 2, 0.3, 128);
    const coinMaterial = new THREE.MeshStandardMaterial({
      color: 0xebb355,
      metalness: 0.2,
      roughness: 0.9,
      transparent: false,
      opacity: 1,
    });
    const coin = new THREE.Mesh(coinGeometry, coinMaterial);
    coinGroup.add(coin);

    const ridgeCount = 180;
    const ridgeGeometry = new THREE.BoxGeometry(0.01, 0.3, 0.01);
    const ridgeMaterial = new THREE.MeshStandardMaterial({
      color: 0xebb355,
      metalness: 0.3,
      roughness: 0.9,
      transparent: false,
      opacity: 1,
    });

    for (let i = 0; i < ridgeCount; i++) {
      const angle = (i / ridgeCount) * Math.PI * 2;
      const ridge = new THREE.Mesh(ridgeGeometry, ridgeMaterial);
      ridge.position.x = Math.cos(angle) * 2;
      ridge.position.z = Math.sin(angle) * 2;
      ridge.rotation.y = angle;
      coinGroup.add(ridge);
    }

    const createFaceDesign = (yPosition: number, rotation: number) => {
      const faceGroup = new THREE.Group();

      const outerRing = new THREE.Mesh(
        new THREE.CylinderGeometry(1.95, 1.95, 0.02, 64),
        new THREE.MeshStandardMaterial({
          color: 0xebb355,
          metalness: 0.2,
          roughness: 0.2,
          transparent: false,
          opacity: 1,
        })
      );
      outerRing.position.y = yPosition;
      outerRing.rotation.x = rotation;
      faceGroup.add(outerRing);

      const purpleCircle = new THREE.Mesh(
        new THREE.CylinderGeometry(1.75, 1.75, 0.03, 64),
        new THREE.MeshStandardMaterial({
          color: 0x836ef9,
          metalness: 0.8,
          roughness: 0.5,
          transparent: false,
          opacity: 1,
        })
      );
      purpleCircle.position.y = yPosition;
      purpleCircle.rotation.x = rotation;
      faceGroup.add(purpleCircle);

      const monadShape = new THREE.Shape();
      const shapeSize = 1.0;
      const radius = 0.1;

      monadShape.moveTo(-shapeSize + radius, -shapeSize);
      monadShape.bezierCurveTo(
        -shapeSize / 2.5,
        -shapeSize - 0.08,
        shapeSize / 2.5,
        -shapeSize - 0.08,
        shapeSize - radius,
        -shapeSize
      );
      monadShape.quadraticCurveTo(
        shapeSize,
        -shapeSize,
        shapeSize,
        -shapeSize + radius
      );
      monadShape.bezierCurveTo(
        shapeSize + 0.08,
        -shapeSize / 2.5,
        shapeSize + 0.08,
        shapeSize / 2.5,
        shapeSize,
        shapeSize - radius
      );
      monadShape.quadraticCurveTo(
        shapeSize,
        shapeSize,
        shapeSize - radius,
        shapeSize
      );
      monadShape.bezierCurveTo(
        shapeSize / 2.5,
        shapeSize + 0.08,
        -shapeSize / 2.5,
        shapeSize + 0.08,
        -shapeSize + radius,
        shapeSize
      );
      monadShape.quadraticCurveTo(
        -shapeSize,
        shapeSize,
        -shapeSize,
        shapeSize - radius
      );
      monadShape.bezierCurveTo(
        -shapeSize - 0.08,
        shapeSize / 2.5,
        -shapeSize - 0.08,
        -shapeSize / 2.5,
        -shapeSize,
        -shapeSize + radius
      );
      monadShape.quadraticCurveTo(
        -shapeSize,
        -shapeSize,
        -shapeSize + radius,
        -shapeSize
      );

      const extrudeSettings = {
        depth: 0.001,
        bevelEnabled: false,
      };

      const monadGeometry = new THREE.ExtrudeGeometry(
        monadShape,
        extrudeSettings
      );
      const monadMaterial = new THREE.MeshStandardMaterial({
        color: 0xf5f5f5,
        metalness: 0.1,
        roughness: 0.4,
        transparent: false,
        opacity: 1,
      });

      const monadLogo = new THREE.Mesh(monadGeometry, monadMaterial);
      monadLogo.rotation.x = rotation === 0 ? -Math.PI / 2 : Math.PI / 2;
      monadLogo.position.y = yPosition + (rotation === 0 ? 0.03 : -0.03);
      monadLogo.rotation.z = Math.PI / 4;

      faceGroup.add(monadLogo);

      const innerShape = new THREE.Shape();
      const innerSize = 0.55;
      const innerRadius = 0.08;

      innerShape.moveTo(-innerSize + innerRadius, -innerSize);
      innerShape.bezierCurveTo(
        -innerSize / 2.5,
        -innerSize - 0.05,
        innerSize / 2.5,
        -innerSize - 0.05,
        innerSize - innerRadius,
        -innerSize
      );
      innerShape.quadraticCurveTo(
        innerSize,
        -innerSize,
        innerSize,
        -innerSize + innerRadius
      );
      innerShape.bezierCurveTo(
        innerSize + 0.05,
        -innerSize / 2.5,
        innerSize + 0.05,
        innerSize / 2.5,
        innerSize,
        innerSize - innerRadius
      );
      innerShape.quadraticCurveTo(
        innerSize,
        innerSize,
        innerSize - innerRadius,
        innerSize
      );
      innerShape.bezierCurveTo(
        innerSize / 2.5,
        innerSize + 0.05,
        -innerSize / 2.5,
        innerSize + 0.05,
        -innerSize + innerRadius,
        innerSize
      );
      innerShape.quadraticCurveTo(
        -innerSize,
        innerSize,
        -innerSize,
        innerSize - innerRadius
      );
      innerShape.bezierCurveTo(
        -innerSize - 0.05,
        innerSize / 2.5,
        -innerSize - 0.05,
        -innerSize / 2.5,
        -innerSize,
        -innerSize + innerRadius
      );
      innerShape.quadraticCurveTo(
        -innerSize,
        -innerSize,
        -innerSize + innerRadius,
        -innerSize
      );

      const innerExtrudeSettings = {
        depth: 0.002,
        bevelEnabled: false,
      };

      const innerGeometry = new THREE.ExtrudeGeometry(
        innerShape,
        innerExtrudeSettings
      );
      const innerMaterial = new THREE.MeshStandardMaterial({
        color: 0x836ef9,
        metalness: 0.8,
        roughness: 0.5,
        transparent: false,
        opacity: 1,
      });

      const innerSquare = new THREE.Mesh(innerGeometry, innerMaterial);
      innerSquare.rotation.x = rotation === 0 ? -Math.PI / 2 : Math.PI / 2;
      innerSquare.position.y = yPosition + (rotation === 0 ? 0.04 : -0.04);
      innerSquare.rotation.z = Math.PI / 5.5;

      faceGroup.add(innerSquare);

      return faceGroup;
    };

    const topFace = createFaceDesign(0.16, 0);
    const bottomFace = createFaceDesign(-0.16, Math.PI);
    coinGroup.add(topFace);
    coinGroup.add(bottomFace);

    scene.add(coinGroup);

    // let rotationVelocityY = 0.005;
    let animationId: number;

    const animate = () => {
      animationId = requestAnimationFrame(animate);

      // targetRotationY += rotationVelocityY;
      // rotationVelocityY *= 0.95;

      const deltaY = 0; // (targetRotationY - coinGroup.rotation.y) * 0.1;
      coinGroup.rotation.y += deltaY;
      coinGroup.rotation.z += 0.05;

      renderer.render(scene, camera);
    };

    animate();

    const cleanup = () => {
      cancelAnimationFrame(animationId);
      renderer.dispose();
      coinGeometry.dispose();
      coinMaterial.dispose();
      ridgeGeometry.dispose();
      ridgeMaterial.dispose();
      if (container.contains(renderer.domElement)) {
        container.removeChild(renderer.domElement);
      }
    };

    sceneRef.current = { scene, camera, renderer, coinGroup, cleanup };

    return cleanup;
  }, [size]);

  return <div ref={containerRef} style={{ width: size, height: size }} />;
}
