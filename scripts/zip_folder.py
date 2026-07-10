import os
import sys
import zipfile
from pathlib import Path

def main():
    if len(sys.argv) < 3:
        print("Usage: python zip_folder.py <source_dir> <output_zip>")
        sys.exit(1)
        
    source_dir = Path(sys.argv[1]).resolve()
    output_zip = Path(sys.argv[2]).resolve()
    if not source_dir.is_dir():
        raise RuntimeError(f"Source directory does not exist: {source_dir}")
    if output_zip.is_relative_to(source_dir):
        raise RuntimeError("Output ZIP must not be created inside the source directory")
    
    print(f"Zipping {source_dir} to {output_zip}...")
    
    if output_zip.exists():
        output_zip.unlink()
        
    # Zip the source_dir placing all its contents inside an 'ApricotPlayer' top-level folder
    with zipfile.ZipFile(output_zip, "w", zipfile.ZIP_DEFLATED) as z:
        for root, dirs, files in os.walk(source_dir):
            for file in files:
                full_path = Path(root) / file
                if full_path.is_symlink():
                    raise RuntimeError(f"Refusing to package symbolic link: {full_path}")
                rel_path = full_path.relative_to(source_dir)
                archive_name = f"ApricotPlayer/{rel_path.as_posix()}"
                z.write(full_path, archive_name)
                
    print("Zipping completed successfully!")

if __name__ == "__main__":
    main()
