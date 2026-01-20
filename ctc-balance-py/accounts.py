"""
Account loader module - loads wallet addresses from text files.
"""

from pathlib import Path
from src.utils import validate_ss58_address


def load_accounts(file_path: str | Path) -> dict[str, str]:
    """
    Load accounts from a text file.
    
    File format:
        # Comment line
        AccountName = WalletAddress
    
    Args:
        file_path: Path to the accounts file
        
    Returns:
        Dict of {name: address}
        
    Raises:
        ValueError: If an invalid SS58 address is found
    """
    accounts = {}
    path = Path(file_path)
    
    if not path.exists():
        raise FileNotFoundError(f"Accounts file not found: {path}")
    
    with open(path, "r") as f:
        for line_num, line in enumerate(f, 1):
            line = line.strip()
            # Skip empty lines and comments
            if not line or line.startswith("#"):
                continue
            
            # Parse "name = address" or "name address" format
            if "=" in line:
                name, address = line.split("=", 1)
                name = name.strip()
                address = address.strip()
            else:
                # Space-separated format: "name address"
                parts = line.split()
                if len(parts) >= 2:
                    name = parts[0]
                    address = parts[1]
                else:
                    continue
            
            # Validate SS58 address (Major Fix: validate input)
            if not validate_ss58_address(address):
                raise ValueError(f"Invalid SS58 address at line {line_num}: {name} = {address}")
            
            accounts[name] = address
    
    return accounts


def load_all_accounts() -> tuple[dict[str, str], dict[str, str], dict[str, str]]:
    """
    Load all accounts from the default files.
    
    Returns:
        Tuple of (my_accounts, happymine_accounts, all_accounts)
    """
    base_dir = Path(__file__).parent
    
    my_accounts = load_accounts(base_dir / "my_accounts.txt")
    happymine_accounts = load_accounts(base_dir / "happymine_accounts.txt")
    all_accounts = {**my_accounts, **happymine_accounts}
    
    return my_accounts, happymine_accounts, all_accounts


# For backward compatibility
def get_all_accounts() -> dict[str, str]:
    """Get all accounts combined."""
    _, _, all_accounts = load_all_accounts()
    return all_accounts
